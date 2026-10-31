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

/// Only a value box drawn this pass can receive the keyboard after a viewport pick.
pub(crate) fn canvas_value_focus(ctx: &egui::Context) -> Option<(egui::Id, usize)> {
    ctx.data(|data| data.get_temp::<(u64, egui::Id, usize)>(egui::Id::new("sc_canvas_value_focus")))
        .filter(|(pass, _, _)| *pass == ctx.cumulative_pass_nr())
        .map(|(_, id, length)| (id, length))
}

fn reference_key() -> egui::Id {
    egui::Id::new("sc_value_reference")
}

/// Put the name `name` into the expression editor `id` when it is next drawn (a dimension
/// clicked in the view, #9477). The editor holds its text, so it does the insertion itself in
/// [`take_reference`]; one it does not draw within a frame drops it.
pub(crate) fn queue_reference(ctx: &egui::Context, id: egui::Id, name: String) {
    let frame = ctx.cumulative_frame_nr();
    ctx.data_mut(|data| data.insert_temp(reference_key(), (frame, id, name)));
    ctx.request_repaint();
}

/// Insert a name queued for this editor into its text, at its cursor or over its selection, a
/// lone value replaced whole (the rule of a dimension's own editor), and give it the keyboard
/// with the cursor after the name. True when the text changed.
pub(crate) fn take_reference(ctx: &egui::Context, id: egui::Id, text: &mut String) -> bool {
    let Some((frame, target, name)) = ctx.data(|data| data.get_temp::<(u64, egui::Id, String)>(reference_key())) else { return false };
    if frame.saturating_add(1) < ctx.cumulative_frame_nr() {
        ctx.data_mut(|data| data.remove::<(u64, egui::Id, String)>(reference_key()));
        return false;
    }
    if target != id {
        return false;
    }
    ctx.data_mut(|data| data.remove::<(u64, egui::Id, String)>(reference_key()));
    let mut state = egui::text_edit::TextEditState::load(ctx, id).unwrap_or_default();
    let cursor;
    (*text, cursor) = crate::dim_view::with_reference(text, &name, state.cursor.char_range());
    state.cursor.set_char_range(Some(egui::text_selection::CCursorRange::one(egui::text::CCursor::new(cursor))));
    state.store(ctx, id);
    ctx.memory_mut(|memory| memory.request_focus(id));
    true
}
