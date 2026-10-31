//! Keyboard focus bookkeeping for a dialog's expression editors. Not a drawn element, so it
//! lives outside the element files the gallery guard checks.

/// Collect the expression editors drawn by a form, in display order.
pub(crate) fn value_fields(ctx: &egui::Context, draw: impl FnOnce() -> bool) -> (bool, Vec<(egui::Id, usize)>) {
    let id = egui::Id::new("sc_value_fields");
    ctx.data_mut(|data| data.insert_temp(id, Vec::<(egui::Id, usize)>::new()));
    let enter = draw();
    let fields = ctx.data_mut(|data| data.remove_temp::<Vec<(egui::Id, usize)>>(id)).unwrap_or_default();
    (enter, fields)
}

/// The expression editor the form drew last so far, in [`value_fields`].
pub(crate) fn last_value_field(ctx: &egui::Context) -> Option<egui::Id> {
    ctx.data(|data| data.get_temp::<Vec<(egui::Id, usize)>>(egui::Id::new("sc_value_fields"))).and_then(|fields| fields.last().map(|(id, _)| *id))
}

/// True while a text editor drawn by the kit holds the keyboard. A dialog uses this to keep at
/// most one field looking active: its pick fields drop their accent ring while a text field
/// has the keyboard (#10066).
pub(crate) fn editor_has_focus(ctx: &egui::Context) -> bool {
    ctx.data(|data| data.get_temp::<egui::Id>(egui::Id::new("sc_focused_editor")))
        .is_some_and(|id| ctx.memory(|memory| memory.focused() == Some(id)))
}

/// Only a value box drawn this pass can receive the keyboard after a viewport pick.
pub(crate) fn canvas_value_focus(ctx: &egui::Context) -> Option<(egui::Id, usize)> {
    ctx.data(|data| data.get_temp::<(u64, egui::Id, usize)>(egui::Id::new("sc_canvas_value_focus")))
        .filter(|(pass, _, _)| *pass == ctx.cumulative_pass_nr())
        .map(|(_, id, length)| (id, length))
}
