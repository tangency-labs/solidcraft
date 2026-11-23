//! A drawing table's editor in its own window in the middle of the app (#18983), not the right panel:
//! one host for every table kind, each kind drawing its own body. The window is the kit's centred
//! card; each frame it takes the width its table asked for, and the card itself stays inside the
//! app window. A revision table's body is its rows ([`super::revision_table::editor`]); a parts
//! list's is its options, as the Parts List tool's panel draws them ([`super::panels::tool_body`]).
use solidcraft_engine::doc::drawing::table::Kind;
use solidcraft_theme::Tokens;

use super::Tool;
use crate::{SolveApp, ui_kit};

/// The open table window: its table and its kind, the width it shows at, and the window it opened
/// in. Every app window draws the drawing's cards, so like the Add Sheet card it paints only there
/// (#20937).
#[derive(Clone, Copy, Debug)]
pub(super) struct TableWindow {
    table: u64,
    shows: Shows,
    width: f32,
    window: solidcraft_views::WindowId,
}

/// The kind of table the window was opened on: it keeps its title when an undo takes the table away.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shows {
    Revision,
    PartsList,
}

impl Shows {
    /// The window's title and the command whose icon it shows.
    fn title(self) -> (&'static str, &'static str) {
        match self {
            Self::Revision => (solidcraft_ui_text::tr("ui.drawing_panel.revision_table"), "drawing.revision_table"),
            Self::PartsList => (solidcraft_ui_text::tr("ui.drawing_panel.parts_list"), "drawing.parts_list"),
        }
    }
}

/// The kind of table `table` of the open drawing, if it has it.
fn kind_of(app: &SolveApp, table: u64) -> Option<Shows> {
    let drawing = app.session.drawing("drawing.inspect").ok()?;
    drawing.tables.by_id(table).map(|placed| match placed.kind {
        Kind::Revision => Shows::Revision,
        Kind::PartsList(_) => Shows::PartsList,
    })
}

/// Open table `table`'s window. A revision table's starts at the width an Add Sheet card does and
/// grows to fit its columns; a parts list's options are rows of a label and a field, at the kit's
/// own width.
pub(super) fn open(app: &mut SolveApp, table: u64) {
    let Some(shows) = kind_of(app, table) else { return };
    let cards = match shows {
        Shows::Revision => 2.5,
        Shows::PartsList => 1.5,
    };
    let window = app.view_windows.current_window();
    app.drawing_workspace.table_window = Some(TableWindow { table, shows, width: Tokens::get().form_card_width * cards, window });
}

/// Whether the open window's grid has a cell edit open. Esc's routing asks before the window is
/// drawn: egui has already taken the focus from the cell's editor by then, so without this the
/// edit's Esc would reach the drawing's own Esc and close the window ([`super::escape_panel`]).
/// (The grid itself keeps the keyboard through Esc, so a range needs nothing here.) Only a revision
/// table's body has cells.
pub(crate) fn editing(app: &SolveApp, ctx: &egui::Context) -> bool {
    super::active(app)
        && app.drawing_workspace.table_window.is_some_and(|open| open.shows == Shows::Revision && open.window == app.view_windows.current_window())
        && super::revision_table::editing(ctx)
}

/// The open table window, if any: its kind's body inside the kit's centred card.
pub(super) fn show(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(mut open) = app.drawing_workspace.table_window.take() else { return };
    // Another window's frame leaves it as it is; leaving the Drawing workspace closes it.
    if open.window != app.view_windows.current_window() {
        app.drawing_workspace.table_window = Some(open);
        return;
    }
    if !super::active(app) {
        return;
    }
    let (title, command) = open.shows.title();
    let icon = solidcraft_engine::find_command(command).map_or("", |c| c.icon);
    // A table an undo took away keeps the window, which says so; its close button closes it.
    let here = kind_of(app, open.table).is_some();
    let mut overflow = 0.0;
    let closed = ui_kit::Window::new(egui::Id::unique(("sc_drawing_table_window", open.table)), icon, title)
        .cards(open.width / Tokens::get().form_card_width)
        .show_body(ctx, |ui| match (here, open.shows) {
            (false, _) => {
                ui_kit::text(ui, solidcraft_ui_text::tr("ui.drawing_table_window.gone"), ui_kit::Text::Note("drawing_table_window_gone"));
            }
            (true, Shows::Revision) => overflow = super::revision_table::editor(app, ui, open.table),
            (true, Shows::PartsList) => super::panels::tool_body(app, ui, Tool::PartsList(Some(open.table))),
        });
    // Esc the grid left (no cell edit or range to cancel) closes the window, as a footer's Cancel
    // would. With nothing focused the drawing's own Esc closes it first ([`super::close_panel`]).
    let closed = closed || (!app.esc_handled && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)));
    if !closed {
        // Columns wider than the card: the card takes that much more next frame (the kit keeps it
        // inside the app window, and the grid scrolls sideways past that).
        if overflow.is_finite() && overflow > 0.5 {
            open.width = (open.width + overflow).min(Tokens::get().form_card_width * 16.0);
        }
        app.drawing_workspace.table_window = Some(open);
    }
}
