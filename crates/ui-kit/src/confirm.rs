//! Modal confirmations share the Panel/Window frame and the single kit footer.
use super::{
    Action, Footer, Window,
    actions::{first_letter_action, footer_with_left},
    text::paint_text,
};
use egui::{Ui, vec2};
use solidcraft_theme::Tokens;

thread_local! {
    /// Does the host let dialog buttons answer the first letter of their labels (#9841)?
    /// Off unless the host turns it on, so the old behaviour — typing does nothing to the
    /// buttons — is the default. See [`set_dialog_mnemonics`].
    static DIALOG_MNEMONICS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Do dialog buttons answer the first letter of their labels? See [`set_dialog_mnemonics`].
pub fn dialog_mnemonics() -> bool {
    DIALOG_MNEMONICS.with(std::cell::Cell::get)
}

/// The host turns the first-letter button accelerators of [`Confirm`] on or off (#9841).
/// Off is the old behaviour: a typed letter does nothing to a dialog's buttons.
pub fn set_dialog_mnemonics(on: bool) {
    DIALOG_MNEMONICS.with(|cell| cell.set(on));
}

#[derive(Clone, Copy)]
pub enum Tone {
    Warning,
    Danger,
    Info,
}

pub struct Confirm<'a> {
    pub window: Window<'a>,
    icon: &'a str,
    title: &'a str,
    tone: Tone,
    ok: &'a str,
    cancel: &'a str,
    left: Option<&'a str>,
    secondary_left: bool,
    left_enabled: bool,
    selected: Option<Action>,
    detail: Option<&'a str>,
}

impl<'a> Confirm<'a> {
    pub fn new(id: egui::Id, icon: &'a str, title: &'a str) -> Self {
        Self {
            window: Window::new(id, icon, title),
            icon,
            title,
            tone: Tone::Info,
            ok: "OK",
            cancel: "Cancel",
            left: None,
            secondary_left: false,
            left_enabled: true,
            selected: None,
            detail: None,
        }
    }

    pub fn tone(mut self, tone: Tone) -> Self {
        self.tone = tone;
        self
    }

    /// Opt into a wider confirmation while keeping the card bounded to the screen.
    pub fn width(mut self, width: f32) -> Self {
        self.window = self.window.width(width);
        self
    }

    pub fn actions(mut self, ok: &'a str, cancel: &'a str, left: Option<&'a str>) -> Self {
        (self.ok, self.cancel, self.left) = (ok, cancel, left);
        self
    }

    /// Frame the optional left action as a secondary button instead of a low-emphasis action.
    pub fn secondary_left(mut self) -> Self {
        self.secondary_left = true;
        self
    }

    /// Keep the left answer visible while it waits for work already in progress.
    pub fn left_enabled(mut self, enabled: bool) -> Self {
        self.left_enabled = enabled;
        self
    }

    pub fn selected(mut self, selected: Action) -> Self {
        self.selected = Some(selected);
        self
    }

    pub fn detail(mut self, detail: &'a str) -> Self {
        self.detail = (!detail.is_empty()).then_some(detail);
        self
    }

    /// The footer's visible buttons with their live states, for the first-letter accelerator:
    /// OK needs valid inputs, Cancel always answers, and the left action has its own state.
    fn accelerators(&self, valid: bool) -> Vec<(Action, &'a str, bool)> {
        let mut out = Vec::new();
        if !self.ok.is_empty() {
            out.push((Action::Ok, self.ok, valid));
        }
        if let Some(left) = self.left {
            out.push((Action::Apply, left, self.left_enabled));
        }
        if !self.cancel.is_empty() {
            out.push((Action::Cancel, self.cancel, true));
        }
        out
    }

    pub fn show(self, ctx: &egui::Context, valid: bool, message: impl FnOnce(&mut Ui)) -> (bool, Action) {
        // Reviewed reports and native pickers temporarily own input. Keep the pending
        // answer untouched and resume the modal when that boundary closes.
        if super::input_blocked() {
            return (false, Action::None);
        }
        self.window.surface(ctx, true, |ui| self.content(ui, valid, message))
    }

    pub fn content(&self, ui: &mut Ui, valid: bool, message: impl FnOnce(&mut Ui)) -> (bool, Action) {
        super::scope(ui, |ui| {
            let t = Tokens::get();
            ui.spacing_mut().item_spacing.y = 0.0;
            // Consume before native button focus can interpret Enter as a different answer.
            let enter = ui.is_enabled() && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
            // #9841: a typed letter answers the button whose translated label starts with it.
            // Read here so the keystroke is consumed even when Escape or a click wins the frame.
            // The host's preference gates the accelerator; off, the keystroke is left alone.
            let accel = if ui.is_enabled() && dialog_mnemonics() { first_letter_action(ui.ctx(), &self.accelerators(valid)) } else { Action::None };
            let mut closed = false;
            egui::Frame::NONE
                .inner_margin(egui::Margin {
                    left: (t.space_16 + t.space_2) as i8,
                    right: (t.space_16 + t.space_2) as i8,
                    top: (t.space_16 + t.space_2) as i8,
                    bottom: t.space_16 as i8,
                })
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = vec2(t.space_14, t.space_6);
                    ui.horizontal_top(|ui| {
                        let diameter = t.control_height + t.space_8;
                        let (disc, _) = ui.allocate_exact_size(vec2(diameter, diameter), egui::Sense::hover());
                        let (fill, accent) = match self.tone {
                            Tone::Warning => (t.warning.gamma_multiply(0.15), Some("construction")),
                            Tone::Danger => (t.form_error_fill, Some("constraint")),
                            Tone::Info => (t.accent_soft, None),
                        };
                        ui.painter().circle_filled(disc.center(), diameter / 2.0, fill);
                        if matches!(self.tone, Tone::Danger) {
                            crate::icons::paint_danger(ui.painter(), disc.shrink(t.space_8), self.icon, 1.0);
                        } else {
                            crate::icons::paint_accented(ui.painter(), disc.shrink(t.space_8), self.icon, accent, 1.0);
                        }
                        solidcraft_ui_registry::publish_rect("confirm:disc", disc);
                        ui.vertical(|ui| {
                            ui.set_width(ui.available_width().max(0.0));
                            let (heading, _) = ui.allocate_exact_size(vec2(ui.available_width().max(0.0), t.form_close_size), egui::Sense::hover());
                            let close = egui::Rect::from_min_size(
                                egui::pos2((heading.right() - t.form_close_size).max(heading.left()), heading.top()),
                                vec2(t.form_close_size, t.form_close_size),
                            );
                            let title = egui::Rect::from_min_max(heading.min, egui::pos2(close.left(), heading.bottom()));
                            paint_text(ui, title, self.title, crate::type_face::semibold(t.font_subtitle), t.form_ink, false);
                            ui.interact(title, ui.scope_id().with("title"), egui::Sense::hover())
                                .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, ui.is_enabled(), self.title));
                            let response = super::icon_button_at(ui, close, "close", "Close");
                            closed = response.clicked();
                            solidcraft_ui_registry::publish_target("dialog:close", response.rect);
                            solidcraft_ui_registry::publish_rect("dialog:header", heading);
                            message(ui);
                            if let Some(detail) = self.detail {
                                super::list_frame(ui, "confirm_detail", |ui| {
                                    super::text(ui, detail, super::Text::MonoNote("confirm_detail"));
                                });
                            }
                        });
                    });
                });
            let action = footer_with_left(
                ui,
                self.ok,
                self.cancel,
                valid,
                Footer {
                    apply: self.left,
                    selected: self.selected,
                    danger: matches!(self.tone, Tone::Danger),
                    confirm: true,
                    secondary_left: self.secondary_left,
                    nothing_to_apply: !self.left_enabled,
                    ..Default::default()
                },
                |_| {},
            );
            let action = if action == Action::Cancel {
                action
            } else if enter && valid && (self.selected != Some(Action::Apply) || self.left_enabled) {
                self.selected.unwrap_or(Action::Ok)
            } else if action == Action::None && accel != Action::None {
                accel
            } else {
                action
            };
            (closed, action)
        })
    }
}

#[cfg(test)]
#[path = "confirm_tests.rs"]
mod tests;
