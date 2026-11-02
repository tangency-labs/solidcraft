//! Right-clicking selected text in an assistant reply keeps the selection and offers Copy.
//!
//! egui 0.36 starts a new label selection on a press of any mouse button (`any_pressed()` in
//! egui's `label_text_selection.rs` does not check which button), so a right-click would clear
//! the selection before any menu could use it. SolidCraft catches the press before egui sees it
//! ([`guard`], run from `SolveApp::raw_input_hook`): when it lands in a reply while a text
//! selection is alive, the press is dropped and a small menu opens at the cursor ([`show`]).
//! Its Copy row copies the selection the same way Cmd+C does: a Copy event pushed into the
//! frame's input, which the reply's labels answer while the selection is still alive.

use egui::{Event, Id, Order, PointerButton, RawInput, Rect};

use crate::SolveApp;
use crate::i18n::tr;
use crate::scenario;
use crate::ui_kit::{Container, Surface};

/// What the guard knew last frame, kept on [`super::AgentChat`].
#[derive(Default)]
pub(crate) struct State {
    /// The assistant replies drawn last frame (global points).
    reply_rect: Option<Rect>,
    /// A label text selection was alive at the end of last frame's layout.
    selection_active: bool,
    /// The menu is open, anchored here (global points).
    menu_at: Option<egui::Pos2>,
    /// The menu as drawn last frame, for the click-inside test.
    menu_rect: Option<Rect>,
}

impl State {
    /// Called at the start of each frame's layout, before the panel draws.
    pub(crate) fn begin_frame(&mut self) {
        self.reply_rect = None;
    }

    /// An assistant reply was drawn at `r` this frame. (Only the desktop panel draws replies.)
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub(crate) fn note_reply(&mut self, r: Rect) {
        self.reply_rect = Some(self.reply_rect.map_or(r, |old| old.union(r)));
    }

    /// The selection as it stood at the end of this frame's layout, for next frame's guard.
    pub(crate) fn note_selection(&mut self, active: bool) {
        self.selection_active = active;
    }

    /// The panel closed: nothing to copy, nothing to offer.
    pub(crate) fn close_menu(&mut self) {
        self.menu_at = None;
        self.menu_rect = None;
    }
}

/// Is a label text selection alive right now?
pub(crate) fn selection_alive(ctx: &egui::Context) -> bool {
    ctx.plugin_opt::<egui::text_selection::LabelSelectionState>().is_some_and(|p| p.lock().has_selection())
}

/// Drop the secondary press that would clear a reply's selection, and answer the menu's Copy.
/// Runs with the frame's input before egui sees it, so the press never reaches label selection.
pub(crate) fn guard(app: &mut SolveApp, raw: &mut RawInput) {
    let st = &mut app.agent_chat.sel_menu;
    let mut drop: Vec<usize> = Vec::new();
    let mut copy = false;
    for (i, event) in raw.events.iter().enumerate() {
        let Event::PointerButton { button, pressed: true, pos, .. } = event else { continue };
        match *button {
            PointerButton::Secondary if st.selection_active && st.reply_rect.is_some_and(|r| r.contains(*pos)) => {
                // egui must not see it: any press starts a new, empty selection.
                st.menu_at = Some(*pos);
                st.menu_rect = None;
                drop.push(i);
            }
            PointerButton::Secondary => st.menu_at = None,
            PointerButton::Primary if st.menu_at.is_some() => {
                if st.menu_rect.is_some_and(|r| r.contains(*pos)) {
                    // The labels answer this frame while the selection is still alive.
                    copy = true;
                    drop.push(i);
                } else {
                    st.menu_at = None;
                }
            }
            _ => {}
        }
    }
    for i in drop.into_iter().rev() {
        raw.events.remove(i);
    }
    if copy {
        st.menu_at = None;
        raw.events.push(Event::Copy);
    }
}

/// Draw the menu above everything, before the reply's labels are laid out this frame.
pub(crate) fn show(app: &mut SolveApp, ctx: &egui::Context) {
    let st = &mut app.agent_chat.sel_menu;
    let Some(at) = st.menu_at else { return };
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) || !selection_alive(ctx) {
        // Escape, a click elsewhere or scrolling ended it: nothing left to copy.
        st.menu_at = None;
        return;
    }
    let resp = Container::new(ctx, Surface::Popup)
        .area(Id::new("sc_agent_chat_selection_menu"))
        .fixed_pos(at)
        .constrain(true)
        .order(Order::Foreground)
        .show(ctx, |ui| {
            crate::context_menu::menu_frame(ui, |ui| {
                crate::context_menu::button(ui, tr("ui.agent_chat_copy"), "copy");
            });
        });
    scenario::publish_rect("agent_chat:copy_menu", resp.response.rect);
    st.menu_rect = Some(resp.response.rect);
}
