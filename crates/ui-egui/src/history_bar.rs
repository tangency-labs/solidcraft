//! The History bar as a component: the grip and title, the History controls, and a strip of step
//! chips with the history marker between them. The app describes each step as plain data
//! ([`Step`]) in the order the bar shows them, draws chips of its own between them through
//! [`Strip::chip`] and [`Strip::inline`] (its [`Contents`]), and runs the [`Action`]s the bar
//! returns. The bar keeps only which chip, or the marker, is being dragged, and reads nothing of
//! the app. Its grip, marker and chips publish their places to the widget registry under the
//! names the app gives ([`Handles`]).
//!
//! Slots count the steps the bar shows: slot `k` is the gap before the `k`-th of them, slot
//! `steps` the end. The marker sits in one; a drop lands in one, on one side of the marker.

use crate::ui_kit::{self, BackingTooltipExt as _, HistoryChip, HistoryStyle, HistoryTransport};
use egui::{Painter, Pos2, Rect, Sense, UiBuilder, pos2, vec2};
use solidcraft_theme::{TabAccent, Tokens};

/// Where the bar publishes its parts in the widget registry.
#[derive(Clone, Copy, Debug)]
pub struct Handles<'a> {
    /// The grip and title, which drag the bar to another dock.
    pub grip: &'a str,
    pub marker: &'a str,
    /// Each step's chip, as this followed by the step's label.
    pub step: &'a str,
    /// Each History control, as this followed by its id (`first`, `back`, `play`...).
    pub controls: &'a str,
}

/// The bar as the app describes it this frame.
#[derive(Clone, Copy, Debug)]
pub struct Bar<'a> {
    /// The heading after the grip, left out when the bar is too narrow for it.
    pub title: &'a str,
    /// The marker's hover text.
    pub marker_tip: &'a str,
    /// The History controls show Play.
    pub play: bool,
    /// History is playing: Play shows Stop.
    pub playing: bool,
    pub accent: TabAccent,
    /// A dragged chip's colour and its drop bar's.
    pub drag_accent: TabAccent,
    /// The marker's slot.
    pub marker: usize,
    /// How many steps the bar shows.
    pub steps: usize,
    /// The floating card, which also dots the last step before the marker.
    pub floating: bool,
    pub handles: Handles<'a>,
}

/// One step's chip.
pub struct Step<'a> {
    pub id: u64,
    /// Its slot: its place among the steps the bar shows.
    pub slot: usize,
    pub label: &'a str,
    /// A kit icon name.
    pub icon: &'a str,
    /// The icon's accent, an icon palette name (`None`: the artwork's own colours).
    pub icon_accent: Option<&'a str>,
    pub state: StepState,
    pub error: bool,
    pub warning: bool,
    /// Draw the icon in this one colour instead of its artwork.
    pub ink: Option<egui::Color32>,
    /// The hover text, written only while it shows.
    pub tip: &'a dyn Fn() -> String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StepState {
    pub selected: bool,
    /// Lit as though hovered (it made what is under the pointer elsewhere).
    pub highlighted: bool,
    /// After the marker: drawn faded.
    pub rolled_back: bool,
    pub suppressed: bool,
    /// The last step before the marker (dotted on the floating card).
    pub last_done: bool,
}

/// Where the marker goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Seek {
    First,
    Back,
    Forward,
    Last,
    /// Dropped in `slot`; `before` says on which side of the marker, at the marker's own slot.
    Slot {
        slot: usize,
        before: bool,
    },
}

/// What the user chose on the bar this frame, in the order it happened.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Seek(Seek),
    /// Play, or Stop while playing.
    Play,
    /// The History controls' menu, opened at `at` (shown by the app, as for [`Action::Menu`]).
    ControlsMenu {
        at: Pos2,
    },
    /// A click on a step, with the modifiers held.
    Select {
        step: u64,
        modifiers: egui::Modifiers,
    },
    /// A double click on a step.
    Edit(u64),
    /// A step's menu, opened at `at`; the app shows its own items in its own menu. A host
    /// without a menu system can draw its items with `ui_kit::Menu` at `at`.
    Menu {
        step: u64,
        at: Pos2,
    },
    /// A step dropped in `slot`; `before` as for [`Seek::Slot`].
    Reorder {
        step: u64,
        slot: usize,
        before: bool,
    },
}

/// The header drawn: where the strip may start, the grip's response, what was chosen.
pub struct HeaderOut {
    pub x: f32,
    pub grip: egui::Response,
    pub actions: Vec<Action>,
}

/// What a History control's response asks for: a click runs it, a right-click opens the
/// controls' menu. (Crate-wide while `HistoryTransport` is.)
pub(crate) fn transport_actions(control: HistoryTransport, response: &egui::Response) -> Vec<Action> {
    let mut actions = Vec::new();
    if response.clicked() {
        actions.push(match control {
            HistoryTransport::First => Action::Seek(Seek::First),
            HistoryTransport::Back => Action::Seek(Seek::Back),
            HistoryTransport::Play => Action::Play,
            HistoryTransport::Forward => Action::Seek(Seek::Forward),
            HistoryTransport::Last => Action::Seek(Seek::Last),
        });
    }
    if response.secondary_clicked() {
        actions.push(Action::ControlsMenu { at: response.interact_pointer_pos().unwrap_or(response.rect.left_bottom()) });
    }
    actions
}

/// Draws the grip, the title when it fits and the History controls along the left of `rect`.
pub fn header(ui: &mut egui::Ui, bar: &Bar, rect: Rect) -> HeaderOut {
    let t = Tokens::get();
    let p = ui.painter().clone();
    let mut x = rect.left() + t.space_10;
    let grip_rect = Rect::from_center_size(pos2(x + t.space_10 / 2.0, rect.center().y), vec2(t.space_10, t.space_16 + t.space_2));
    let mut grip_ui = ui.new_child(UiBuilder::new().max_rect(grip_rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
    let grip = ui_kit::panel_grip(&mut grip_ui).rect;
    x += t.space_10 + t.space_8;
    let mut drag_rect = grip;
    let heading = ui_kit::StripHeading::new(&p, bar.title);
    if x + heading.width() + ui_kit::history_transport_width() <= rect.right() {
        let title_rect = heading.paint(&p, Rect::from_min_max(pos2(x, rect.top()), rect.right_bottom()));
        drag_rect = drag_rect.union(title_rect);
        x += heading.width();
    }
    let grip = ui.interact(drag_rect, ui.scope_id().with(("history_bar_grip", bar.handles.grip)), Sense::drag());
    solidcraft_ui_registry::publish_handle(bar.handles.grip, drag_rect.center());
    let mut actions = Vec::new();
    for &control in HistoryTransport::shown(bar.play) {
        let br = Rect::from_center_size(pos2(x + t.space_24 / 2.0, rect.center().y), vec2(t.space_24, t.space_24));
        let response = control.show_as(ui, br, bar.playing, bar.handles.controls);
        actions.extend(transport_actions(control, &response));
        x += t.space_24 + t.space_2;
    }
    HeaderOut { x: x + 14.0, grip, actions }
}

/// What the app draws in the strip.
pub trait Contents {
    /// Lays out the strip's entries in order: steps, the app's own chips and trays.
    fn entries(&mut self, strip: &mut Strip<'_, '_>);

    /// Draws what follows the steps and the marker at the end, from `end`; returns where it ends.
    fn trailing(&mut self, _ui: &mut egui::Ui, _painter: &Painter, end: f32, _bar: Rect) -> f32 {
        end
    }
}

/// The strip drawn: what was chosen, and the step under the pointer.
#[derive(Debug, Default)]
pub struct StripOut {
    pub actions: Vec<Action>,
    pub hovered: Option<u64>,
}

#[derive(Clone, Debug, PartialEq)]
enum Drag {
    Marker,
    Step(u64),
}

/// The strip being laid out, left to right.
pub struct Strip<'u, 'b> {
    ui: &'u mut egui::Ui,
    painter: Painter,
    bar: Bar<'b>,
    /// The whole bar's rect: chips sit at its top.
    rect: Rect,
    x: f32,
    /// Each slot's left edge, one per step (a folded chip repeats its own for the steps it holds).
    xs: Vec<f32>,
    marker_rect: Option<Rect>,
    /// Each open tray: where it starts, its place under the chips, its depth.
    trays: Vec<(f32, egui::layers::ShapeIdx, usize)>,
    drag: Option<Drag>,
    hovered: Option<u64>,
    edit: Option<u64>,
    actions: Vec<Action>,
}

/// Draws the steps in `rect`, scrolled sideways (`scroll` keeps its own scroll offset), the bar's
/// rect being `bar_rect`; the drop of a dragged chip or marker is returned when it is released.
pub fn strip(
    ui: &mut egui::Ui,
    bar: &Bar,
    bar_rect: Rect,
    rect: Rect,
    scroll: impl std::hash::Hash + std::fmt::Debug,
    contents: &mut dyn Contents,
) -> StripOut {
    let mut out = StripOut::default();
    ui.scope_builder(UiBuilder::new().max_rect(rect), |ui| {
        // A plain mouse wheel scrolls the strip sideways too (#3356).
        ui.style_mut().always_scroll_the_only_direction = true;
        let area = egui::ScrollArea::horizontal().id_salt(scroll).auto_shrink([false, false]);
        area.show(ui, |ui| out = lay_out(ui, *bar, bar_rect, contents));
    });
    out
}

fn lay_out(ui: &mut egui::Ui, bar: Bar, rect: Rect, contents: &mut dyn Contents) -> StripOut {
    let t = Tokens::get();
    let drag_id = ui.scope_id().with("tl_drag");
    let drag: Option<Drag> = ui.data(|d| d.get_temp(drag_id));
    let pointer = ui.input(|i| i.pointer.hover_pos());
    let x = ui.max_rect().left();
    let origin = x;
    let painter = ui.painter().clone();
    let mut strip = Strip {
        ui,
        painter,
        bar,
        rect,
        x,
        xs: Vec::new(),
        marker_rect: None,
        trays: Vec::new(),
        drag,
        hovered: None,
        edit: None,
        actions: Vec::new(),
    };
    contents.entries(&mut strip);
    let Strip { ui, painter: p, x: end, xs, mut marker_rect, mut drag, hovered, edit, mut actions, .. } = strip;
    let marker = bar.marker;
    if marker >= bar.steps && marker_rect.is_none() {
        marker_rect = Some(marker_at(rect, end));
    }
    // The history marker: drag it to roll the model back or forward.
    if let Some(mr) = marker_rect {
        solidcraft_ui_registry::publish_rect(bar.handles.marker, mr);
        let resp = ui.interact(mr.expand2(vec2(3.0, 0.0)), ui.scope_id().with("tl_marker"), Sense::drag());
        if resp.drag_started() {
            drag = Some(Drag::Marker);
        }
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
        ui_kit::history_marker(&p, mr, false, bar.accent);
        resp.backing_on_hover_text(bar.marker_tip);
    }
    let content_end = contents.trailing(ui, &p, end, rect);
    // The scroll range follows the drawn content (the chips don't take layout space themselves).
    ui.allocate_exact_size(vec2((content_end - origin).max(0.0), rect.height()), Sense::hover());
    // Drag feedback, and the drop.
    if drag.is_some() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    }
    // At the marker's slot, which side of the marker the pointer is on: before it, a dropped
    // step stays in the model; after it, the step is rolled back.
    let before_marker = |slot: usize, x: f32| slot < marker || (slot == marker && marker_rect.is_none_or(|mr| x < mr.center().x));
    // The marker dropped where it already is needs no bar: the marker shows the spot.
    if let (Some(dg), Some(pp)) = (drag.as_ref(), pointer)
        && let local = slot_at(&xs, end, pp.x)
        && !(matches!(dg, Drag::Marker) && local == marker)
    {
        // Centred in the gap between two chips, or beside the marker at its slot.
        let sx = match marker_rect {
            Some(mr) if local == marker => {
                if before_marker(local, pp.x) {
                    mr.left() - 2.0
                } else {
                    mr.right() + 2.0
                }
            }
            _ => xs.get(local).copied().unwrap_or(end) - 2.0,
        };
        // Kept inside the strip so the slot before the first step stays visible.
        let sx = sx.max(origin + t.space_2);
        ui_kit::history_drop_bar(
            &p,
            Rect::from_min_max(pos2(sx, rect.top() + 3.0), pos2(sx, rect.bottom() - 3.0)),
            false,
            if matches!(dg, Drag::Marker) { bar.accent.color } else { bar.drag_accent.color },
        );
    }
    let released = ui.input(|i| i.pointer.any_released());
    // (Taking the drag ends it even when the pointer has left the window.)
    if released
        && let Some(dg) = drag.take()
        && let Some(pp) = pointer
    {
        let slot = slot_at(&xs, end, pp.x);
        let before = before_marker(slot, pp.x);
        actions.push(match dg {
            Drag::Marker => Action::Seek(Seek::Slot { slot, before }),
            Drag::Step(step) => Action::Reorder { step, slot, before },
        });
    }
    ui.data_mut(|d| match drag {
        Some(dg) => {
            d.insert_temp(drag_id, dg);
        }
        None => d.remove::<Drag>(drag_id),
    });
    actions.extend(edit.map(Action::Edit));
    StripOut { actions, hovered }
}

/// The marker's rect at `x` in the bar `rect`.
fn marker_at(rect: Rect, x: f32) -> Rect {
    let t = Tokens::get();
    Rect::from_min_size(pos2(x, rect.top() + 5.0), vec2(t.space_12, rect.height() - 10.0))
}

/// Slot index (0..=n) nearest to screen x, given the left edges of the chips.
fn slot_at(xs: &[f32], end: f32, x: f32) -> usize {
    let mut best = (0usize, f32::INFINITY);
    for (i, sx) in xs.iter().chain(std::iter::once(&end)).enumerate() {
        let d = (sx - 2.0 - x).abs();
        if d < best.1 {
            best = (i, d);
        }
    }
    best.0
}

impl Strip<'_, '_> {
    /// The strip's ui, for what the app draws itself.
    pub fn ui(&mut self) -> &mut egui::Ui {
        self.ui
    }

    pub fn painter(&self) -> &Painter {
        &self.painter
    }

    /// The whole bar's rect.
    pub fn rect(&self) -> Rect {
        self.rect
    }

    /// Puts the marker here once.
    fn marker_here(&mut self) {
        if self.marker_rect.is_none() {
            self.marker_rect = Some(marker_at(self.rect, self.x));
            self.x += Tokens::get().space_16;
        }
    }

    /// Draws a step's chip and returns its rect. A chip scrolled out of the strip is neither
    /// drawn nor interactive; its slot still places the marker and drops. One being dragged or
    /// focused, or under an open menu, stays alive until that ends.
    pub fn step(&mut self, step: &Step) -> Rect {
        let t = Tokens::get();
        if step.slot == self.bar.marker {
            self.marker_here();
        }
        self.xs.push(self.x);
        let br = Rect::from_min_size(pos2(self.x, self.rect.top() + 6.0), vec2(28.0, 28.0));
        self.x += 32.0;
        solidcraft_ui_registry::publish_handle(&format!("{}{}", self.bar.handles.step, step.label), br.center());
        let ui = &mut *self.ui;
        let chip_id = ui.scope_id().with(("feat", step.id));
        let dragged = self.drag == Some(Drag::Step(step.id));
        if !ui.clip_rect().intersects(br) && !dragged && !ui.memory(|m| m.has_focus(chip_id)) && !egui::Popup::is_any_open(ui.ctx()) {
            return br;
        }
        let resp = ui.interact(br, chip_id, Sense::click_and_drag());
        if resp.hovered() {
            self.hovered = Some(step.id);
        }
        HistoryChip {
            icon: step.icon,
            name: step.label,
            kind: "",
            selected: step.state.selected,
            hovered: resp.hovered() || step.state.highlighted,
            dim: step.state.rolled_back,
            suppressed: step.state.suppressed,
            error: step.error,
            warning: step.warning,
            reference: false,
            ink: step.ink,
            accent: if dragged { self.bar.drag_accent } else { self.bar.accent },
        }
        .paint_with_accent(&self.painter, br, HistoryStyle::Bottom, step.icon_accent);
        if self.bar.floating && step.state.last_done {
            self.painter.circle_filled(pos2(br.center().x, br.bottom() + t.space_3), t.space_2, self.bar.accent.color);
        }
        if resp.drag_started() {
            self.drag = Some(Drag::Step(step.id));
        }
        let resp = resp.backing_on_hover_ui(|ui| {
            ui.set_max_width(ui.spacing().tooltip_width);
            ui.add(egui::Label::new((step.tip)()));
        });
        if resp.clicked() {
            self.actions.push(Action::Select { step: step.id, modifiers: ui.input(|i| i.modifiers) });
        }
        if resp.double_clicked() {
            self.edit = Some(step.id);
        }
        if resp.secondary_clicked() {
            self.actions.push(Action::Menu { step: step.id, at: resp.interact_pointer_pos().unwrap_or(br.center()) });
        }
        br
    }

    /// A chip of the app's own for the steps in slots `start..end` (a folded run or folder), drawn
    /// by `draw` with its top left at the given point, returning its width. The marker goes before
    /// it at its first slot, and after it inside a folded one; a drop on a folded chip lands
    /// before it. An `open` one starts a tray `depth` deep, ended by [`Strip::end_tray`].
    /// Returns its rect.
    pub fn chip(&mut self, start: usize, end: usize, open: bool, depth: usize, draw: impl FnOnce(&mut egui::Ui, &Painter, Pos2) -> f32) -> Rect {
        let marker = self.bar.marker;
        if start < end && start == marker {
            self.marker_here();
        }
        if open {
            self.trays.push((self.x, self.painter.add(egui::Shape::Noop), depth));
            self.x += ui_kit::HISTORY_TRAY_PAD;
        }
        let at = pos2(self.x, self.rect.top() + 6.0);
        let width = draw(self.ui, &self.painter, at);
        if !open {
            self.xs.extend(std::iter::repeat_n(self.x, end.saturating_sub(start)));
        }
        self.x += width + 4.0;
        if !open && start < marker && marker < end {
            self.marker_here();
        }
        Rect::from_min_size(at, vec2(width, 28.0))
    }

    /// Ends the innermost open tray here.
    pub fn end_tray(&mut self) {
        if let Some((left, tray, depth)) = self.trays.pop() {
            let right = self.x - 4.0 + ui_kit::HISTORY_TRAY_PAD;
            self.painter.set(tray, ui_kit::history_tray(left, right, self.rect.top() + 6.0, depth, self.bar.accent));
            self.x = right + 4.0;
        }
    }

    /// Something of the app's own that takes no slot, drawn by `draw` from x in the bar's rect,
    /// returning where it ends.
    pub fn inline(&mut self, draw: impl FnOnce(&mut egui::Ui, &Painter, f32, Rect) -> f32) {
        self.x = draw(self.ui, &self.painter, self.x, self.rect);
    }
}

#[cfg(test)]
#[path = "history_bar_tests.rs"]
mod tests;
