//! Shared review composition for operations that replace saved rendering state.
use super::{Action, Confirm, Sphere, Text, checkbox, list_frame, sphere_thumbnail, text};
use crate::i18n::tr;

/// Content varies; the card, impact rows, acknowledgement and modal actions do not.
pub(crate) struct ReviewAndApply<'a> {
    pub title: &'a str,
    pub name: &'a str,
    pub subtitle: &'a str,
    pub replaces: &'a str,
    pub kept: &'a str,
    pub acknowledgement: &'a str,
    pub apply: &'a str,
    pub copy: bool,
    pub notice: Option<&'a str>,
}

impl ReviewAndApply<'_> {
    pub(crate) fn show(&self, ctx: &egui::Context, valid: bool, acknowledged: &mut bool, content: impl FnOnce(&mut egui::Ui)) -> (bool, Action) {
        Confirm::new(egui::Id::new("render_review_apply"), "inspect", self.title)
            .actions(self.apply, tr("ui.cancel"), self.copy.then(|| tr("render.physical_copy_plan")))
            .secondary_left()
            .show(ctx, valid && *acknowledged, |ui| self.content(ui, acknowledged, content))
    }

    pub(crate) fn content(&self, ui: &mut egui::Ui, acknowledged: &mut bool, content: impl FnOnce(&mut egui::Ui)) {
        list_frame(ui, "review:card", |ui| {
            ui.horizontal(|ui| {
                sphere_thumbnail(
                    ui,
                    "review:sphere",
                    Sphere { color: crate::theme::Tokens::get().body, kind: super::FinishKind::Satin, opacity: None },
                    40.0,
                );
                ui.vertical(|ui| {
                    text(ui, self.name, Text::Heading("review:name"));
                    text(ui, self.subtitle, Text::Note("review:subtitle"));
                });
            });
        });
        if let Some(notice) = self.notice {
            let t = crate::theme::Tokens::get();
            crate::ui_kit::Container::show_backing(
                ui,
                egui::Frame::NONE.fill(t.accent_soft).corner_radius(t.control_radius).inner_margin(egui::Margin::same(t.space_8 as i8)),
                |ui| {
                    text(ui, notice, Text::Note("review:physical"));
                },
            );
        }
        content(ui);
        for (label, value, key) in
            [(tr("render.stage_panels.replaces"), self.replaces, "review:replaces"), (tr("render.stage_panels.kept"), self.kept, "review:kept")]
        {
            // Impact text wraps independently of ordinary one-line form rows.
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(crate::theme::Tokens::get().space_16 * 4.0);
                    text(ui, label, Text::Inline);
                });
                ui.vertical(|ui| {
                    ui.set_width(ui.available_width());
                    text(ui, value, Text::Note(key));
                });
            });
        }
        let ack = checkbox(ui, self.acknowledgement, acknowledged);
        crate::scenario::publish_target("render:review:acknowledge", ack.rect);
    }
}
