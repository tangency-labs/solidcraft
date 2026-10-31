//! Keyboard focus bookkeeping for a dialog's expression editors. Not a drawn element, so it
//! lives outside the element files the gallery guard checks.

/// Collect the expression editors drawn by a form, in display order.
pub(crate) fn value_fields(ctx: &egui::Context, draw: impl FnOnce() -> bool) -> (bool, Vec<(egui::Id, usize)>) {
    let id = egui::Id::unique("sc_value_fields");
    ctx.data_mut(|data| data.insert_temp(id, Vec::<(egui::Id, usize)>::new()));
    let enter = draw();
    let fields = ctx.data_mut(|data| data.remove_temp::<Vec<(egui::Id, usize)>>(id)).unwrap_or_default();
    (enter, fields)
}

/// The expression editor the form drew last so far, in [`value_fields`].
pub(crate) fn last_value_field(ctx: &egui::Context) -> Option<egui::Id> {
    ctx.data(|data| data.get_temp::<Vec<(egui::Id, usize)>>(egui::Id::unique("sc_value_fields"))).and_then(|fields| fields.last().map(|(id, _)| *id))
}

/// Only a value box drawn this pass can receive the keyboard after a viewport pick.
pub(crate) fn canvas_value_focus(ctx: &egui::Context) -> Option<(egui::Id, usize)> {
    ctx.data(|data| data.get_temp::<(u64, egui::Id, usize)>(egui::Id::unique("sc_canvas_value_focus")))
        .filter(|(pass, _, _)| *pass == ctx.cumulative_pass_nr())
        .map(|(_, id, length)| (id, length))
}
