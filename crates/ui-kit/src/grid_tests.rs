use super::*;
use egui::{Event, Key, Modifiers};
use solidcraft_ui_registry as registry;

/// A small in-memory table: rows of three cells (Name and Value editable, Note read-only).
struct Sheet {
    keys: Vec<String>,
    cells: Vec<[String; 3]>,
    /// Paste may add rows below the last.
    grows: bool,
    /// Value refuses to be empty.
    value_required: bool,
    /// Cells built this frame.
    drawn: usize,
}

impl Sheet {
    fn new(rows: usize) -> Self {
        Sheet {
            keys: (0..rows).map(|r| format!("r{r}")).collect(),
            cells: (0..rows).map(|r| [format!("name{r}"), format!("{r} mm"), format!("note{r}")]).collect(),
            grows: false,
            value_required: false,
            drawn: 0,
        }
    }

    /// Apply one gesture's edits, as the app runs one command for them.
    fn apply(&mut self, edits: &[Edit]) {
        // New rows count from the rows there were when the gesture was made.
        let base = self.keys.len();
        for edit in edits {
            let row = match edit.row() {
                RowRef::Existing(key) => self.keys.iter().position(|k| k == key).unwrap(),
                RowRef::New(n) => {
                    while self.keys.len() <= base + n {
                        self.keys.push(format!("new{}", self.keys.len()));
                        self.cells.push(Default::default());
                    }
                    base + n
                }
            };
            self.cells[row][edit.col()] = edit.text().to_owned();
        }
    }

    fn cell(&self, row: usize, col: usize) -> &str {
        &self.cells[row][col]
    }
}

impl GridSource for Sheet {
    fn columns(&self) -> Vec<GridColumn> {
        vec![
            GridColumn::new("name", "Name", 1.0, CellKind::Text).flex(),
            GridColumn::new("value", "Value", 1.0, CellKind::Text).flex(),
            GridColumn::new("note", "Note", 1.0, CellKind::ReadOnly).flex(),
        ]
    }
    fn row_count(&self) -> usize {
        self.keys.len()
    }
    fn row_key(&self, row: usize) -> Cow<'_, str> {
        Cow::Borrowed(&self.keys[row])
    }
    fn text(&self, row: usize, col: usize) -> Cow<'_, str> {
        Cow::Borrowed(&self.cells[row][col])
    }
    fn set_cell(&mut self, _row: usize, col: usize, text: &str) -> Result<(), String> {
        if self.value_required && col == 1 && text.trim().is_empty() { Err("needs a value".into()) } else { Ok(()) }
    }
    fn add_row(&self) -> Option<Cow<'_, str>> {
        self.grows.then_some(Cow::Borrowed("Add row"))
    }
    fn drawn(&mut self, _ui: &Ui, _row: usize, _col: usize, _response: &Response, _editing: bool) {
        self.drawn += 1;
    }
}

const COMMAND: Modifiers = Modifiers { alt: false, ctrl: true, shift: false, mac_cmd: false, command: true };

/// One frame of the grid `g` over `sheet`, with `events` and `modifiers` held.
fn run(ctx: &egui::Context, sheet: &mut Sheet, events: Vec<Event>, modifiers: Modifiers) -> (GridOutput, egui::FullOutput) {
    registry::clear_handles();
    sheet.drawn = 0;
    let mut out = GridOutput::default();
    let mut held = vec![Event::ModifiersChanged(modifiers)];
    held.extend(events);
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(480.0, 400.0))),
        events: held,
        focused: true,
        ..Default::default()
    };
    let full = crate::test_support::raw_frame(ctx, input, |ui| {
        crate::scope(ui, |ui| out = Grid::new("g").show(ui, sheet));
    });
    (out, full)
}

fn frame(ctx: &egui::Context, sheet: &mut Sheet, events: Vec<Event>) -> GridOutput {
    run(ctx, sheet, events, Modifiers::NONE).0
}

fn key_with(key: Key, modifiers: Modifiers) -> Vec<Event> {
    vec![
        Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers },
        Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers },
    ]
}

/// Press `key` with `modifiers`, apply what the gesture handed back, and let the grid settle.
fn press(ctx: &egui::Context, sheet: &mut Sheet, key: Key, modifiers: Modifiers) -> GridOutput {
    let (out, _) = run(ctx, sheet, key_with(key, modifiers), modifiers);
    sheet.apply(&out.edits);
    frame(ctx, sheet, vec![]);
    out
}

fn centre(sheet_row: &str, column: &str) -> egui::Pos2 {
    registry::rect_of(&format!("grid:g:{sheet_row}:{column}")).unwrap_or_else(|| panic!("no cell {sheet_row}:{column}")).center()
}

fn click(ctx: &egui::Context, sheet: &mut Sheet, at: egui::Pos2, modifiers: Modifiers) {
    let events = vec![
        Event::PointerMoved(at),
        Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: true, modifiers },
        Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: false, modifiers },
    ];
    run(ctx, sheet, events, modifiers);
    frame(ctx, sheet, vec![]);
}

fn selection(ctx: &egui::Context) -> (usize, usize, usize, usize) {
    Grid::selection(ctx, Grid::state_of("g")).unwrap()
}

/// A sheet of `rows`, drawn once, with the cursor on its first cell and the grid holding the keyboard.
fn focused(rows: usize) -> (egui::Context, Sheet) {
    let ctx = crate::test_support::context();
    let mut sheet = Sheet::new(rows);
    frame(&ctx, &mut sheet, vec![]);
    click(&ctx, &mut sheet, centre("r0", "name"), Modifiers::NONE);
    assert_eq!(selection(&ctx), (0, 0, 0, 0), "a click puts the cursor on the cell");
    (ctx, sheet)
}

fn copied(full: &egui::FullOutput) -> Option<String> {
    full.platform_output.commands.iter().find_map(|c| match c {
        egui::OutputCommand::CopyText(text) => Some(text.clone()),
        _ => None,
    })
}

#[test]
fn arrows_tab_enter_home_and_end_move_the_cursor() {
    let (ctx, mut sheet) = focused(4);
    let at = |ctx: &egui::Context| {
        let (r, _, c, _) = selection(ctx);
        (r, c)
    };
    press(&ctx, &mut sheet, Key::ArrowDown, Modifiers::NONE);
    press(&ctx, &mut sheet, Key::ArrowRight, Modifiers::NONE);
    assert_eq!(at(&ctx), (1, 1));
    press(&ctx, &mut sheet, Key::ArrowRight, Modifiers::NONE);
    press(&ctx, &mut sheet, Key::ArrowRight, Modifiers::NONE);
    assert_eq!(at(&ctx), (1, 2), "arrows stop at the row's end");
    press(&ctx, &mut sheet, Key::Tab, Modifiers::NONE);
    assert_eq!(at(&ctx), (2, 0), "Tab wraps to the next row");
    press(&ctx, &mut sheet, Key::Tab, Modifiers::SHIFT);
    assert_eq!(at(&ctx), (1, 2), "Shift+Tab wraps back");
    press(&ctx, &mut sheet, Key::Enter, Modifiers::NONE);
    assert_eq!(at(&ctx), (2, 2), "Enter goes down the column");
    press(&ctx, &mut sheet, Key::Enter, Modifiers::SHIFT);
    press(&ctx, &mut sheet, Key::Enter, Modifiers::SHIFT);
    assert_eq!(at(&ctx), (0, 2), "Shift+Enter goes up");
    press(&ctx, &mut sheet, Key::Home, Modifiers::NONE);
    assert_eq!(at(&ctx), (0, 0));
    press(&ctx, &mut sheet, Key::End, COMMAND);
    assert_eq!(at(&ctx), (3, 2), "Ctrl+End: the table's last cell");
    press(&ctx, &mut sheet, Key::ArrowUp, COMMAND);
    assert_eq!(at(&ctx), (0, 2), "Ctrl+Up: the column's top");
    assert!(sheet.cells.iter().enumerate().all(|(r, c)| c[0] == format!("name{r}")), "moving changes nothing");
}

#[test]
fn shift_arrows_shift_click_and_drag_select_ranges() {
    let (ctx, mut sheet) = focused(5);
    press(&ctx, &mut sheet, Key::ArrowDown, Modifiers::SHIFT);
    press(&ctx, &mut sheet, Key::ArrowRight, Modifiers::SHIFT);
    assert_eq!(selection(&ctx), (0, 1, 0, 1), "Shift+arrows grow the range from the anchor");
    press(&ctx, &mut sheet, Key::ArrowDown, Modifiers::NONE);
    assert_eq!(selection(&ctx), (2, 2, 1, 1), "a plain arrow collapses it");
    click(&ctx, &mut sheet, centre("r4", "note"), Modifiers::SHIFT);
    assert_eq!(selection(&ctx), (2, 4, 1, 2), "Shift+click extends from the anchor");
    // Press on one cell, move to another with the button down, release.
    let (from, to) = (centre("r1", "name"), centre("r3", "value"));
    run(
        &ctx,
        &mut sheet,
        vec![
            Event::PointerMoved(from),
            Event::PointerButton { pos: from, button: egui::PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE },
        ],
        Modifiers::NONE,
    );
    for step in 1..=4 {
        let at = from + (to - from) * (step as f32 / 4.0);
        run(&ctx, &mut sheet, vec![Event::PointerMoved(at)], Modifiers::NONE);
    }
    run(
        &ctx,
        &mut sheet,
        vec![Event::PointerButton { pos: to, button: egui::PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE }],
        Modifiers::NONE,
    );
    frame(&ctx, &mut sheet, vec![]);
    assert_eq!(selection(&ctx), (1, 3, 0, 1), "a drag selects the cells it crosses");
    assert!(Grid::editing(&ctx, Grid::state_of("g")).is_none(), "a drag never opens an editor");
    press(&ctx, &mut sheet, Key::A, COMMAND);
    assert_eq!(selection(&ctx), (0, 4, 0, 2), "Ctrl+A selects everything");
}

#[test]
fn copy_writes_tsv_and_an_excel_block_pastes_back_as_one_gesture() {
    let (ctx, mut sheet) = focused(4);
    sheet.cells[1][0] = "has\ttab".into();
    frame(&ctx, &mut sheet, vec![]);
    press(&ctx, &mut sheet, Key::ArrowDown, Modifiers::SHIFT);
    press(&ctx, &mut sheet, Key::ArrowRight, Modifiers::SHIFT);
    press(&ctx, &mut sheet, Key::ArrowRight, Modifiers::SHIFT);
    let (_, full) = run(&ctx, &mut sheet, key_with(Key::C, COMMAND), COMMAND);
    let text = copied(&full).expect("Ctrl+C puts the range on the clipboard");
    assert_eq!(text, "name0\t0 mm\tnote0\r\n\"has\ttab\"\t1 mm\tnote1");
    // Paste that text, as Excel hands it back (with its trailing CRLF), at row 2.
    click(&ctx, &mut sheet, centre("r2", "name"), Modifiers::NONE);
    let (out, _) = run(&ctx, &mut sheet, vec![Event::Paste(format!("{text}\r\n"))], Modifiers::NONE);
    assert_eq!(out.gesture, Some(Gesture::Paste));
    let set = |row: &str, col: usize, text: &str| Edit::Set { row: RowRef::Existing(row.into()), col, text: text.into() };
    assert_eq!(
        out.edits,
        [set("r2", 0, "name0"), set("r2", 1, "0 mm"), set("r3", 0, "has\ttab"), set("r3", 1, "1 mm")],
        "one list, read-only Note skipped"
    );
    sheet.apply(&out.edits);
    assert_eq!((sheet.cell(3, 0), sheet.cell(3, 1), sheet.cell(3, 2)), ("has\ttab", "1 mm", "note3"));
    frame(&ctx, &mut sheet, vec![]);
    assert_eq!(selection(&ctx), (2, 3, 0, 2), "the pasted block becomes the range");
}

#[test]
fn a_paste_past_the_last_row_adds_rows_only_when_the_source_can() {
    let (ctx, mut sheet) = focused(2);
    click(&ctx, &mut sheet, centre("r1", "name"), Modifiers::NONE);
    let block = "a\t1\nb\t2\nc\t3".to_owned();
    let (out, _) = run(&ctx, &mut sheet, vec![Event::Paste(block.clone())], Modifiers::NONE);
    assert_eq!(out.edits.len(), 2, "only the last row takes the paste: {:?}", out.edits);
    assert!(out.notice.is_some(), "the cut-off is said");
    sheet.grows = true;
    frame(&ctx, &mut sheet, vec![]);
    let (out, _) = run(&ctx, &mut sheet, vec![Event::Paste(block)], Modifiers::NONE);
    assert!(out.notice.is_none());
    let new: Vec<_> =
        out.edits.iter().filter(|e| matches!(e.row(), RowRef::New(_))).map(|e| (e.row().clone(), e.col(), e.text().to_owned())).collect();
    assert_eq!(
        new,
        [(RowRef::New(0), 0, "b".into()), (RowRef::New(0), 1, "2".into()), (RowRef::New(1), 0, "c".into()), (RowRef::New(1), 1, "3".into())]
    );
}

#[test]
fn one_value_pasted_over_a_range_fills_it() {
    let (ctx, mut sheet) = focused(3);
    press(&ctx, &mut sheet, Key::ArrowRight, Modifiers::NONE);
    press(&ctx, &mut sheet, Key::ArrowDown, Modifiers::SHIFT);
    press(&ctx, &mut sheet, Key::ArrowDown, Modifiers::SHIFT);
    let (out, _) = run(&ctx, &mut sheet, vec![Event::Paste("9 mm\r\n".into())], Modifiers::NONE);
    sheet.apply(&out.edits);
    assert_eq!(out.edits.len(), 3);
    assert!((0..3).all(|r| sheet.cell(r, 1) == "9 mm"));
}

#[test]
fn ctrl_d_fills_the_top_row_down_as_one_gesture() {
    let (ctx, mut sheet) = focused(4);
    press(&ctx, &mut sheet, Key::ArrowRight, Modifiers::NONE);
    for _ in 0..3 {
        press(&ctx, &mut sheet, Key::ArrowDown, Modifiers::SHIFT);
    }
    let out = press(&ctx, &mut sheet, Key::D, COMMAND);
    assert_eq!(out.gesture, Some(Gesture::Fill));
    assert_eq!(out.edits.len(), 3, "rows 1 to 3 take row 0's value: {:?}", out.edits);
    assert!((0..4).all(|r| sheet.cell(r, 1) == "0 mm"));
    // A one-row range takes the row above.
    press(&ctx, &mut sheet, Key::ArrowLeft, Modifiers::NONE);
    sheet.cells[2][0] = "above".into();
    let out = press(&ctx, &mut sheet, Key::D, COMMAND);
    assert_eq!(out.edits, [Edit::Set { row: RowRef::Existing("r3".into()), col: 0, text: "above".into() }]);
}

#[test]
fn typing_edits_enter_commits_and_moves_escape_cancels() {
    let (ctx, mut sheet) = focused(3);
    press(&ctx, &mut sheet, Key::ArrowRight, Modifiers::NONE);
    frame(&ctx, &mut sheet, vec![Event::Text("4".into())]);
    frame(&ctx, &mut sheet, vec![Event::Text("2 mm".into())]);
    assert_eq!(Grid::editing(&ctx, Grid::state_of("g")), Some(("r0".into(), 1, "42 mm".into())), "typing replaced the cell");
    let out = press(&ctx, &mut sheet, Key::Enter, Modifiers::NONE);
    assert_eq!(out.edits, [Edit::Set { row: RowRef::Existing("r0".into()), col: 1, text: "42 mm".into() }]);
    assert!(out.by_key);
    assert_eq!(sheet.cell(0, 1), "42 mm");
    assert_eq!(selection(&ctx), (1, 1, 1, 1), "and moved down");
    // F2 opens with the text selected; Escape cancels and keeps the value.
    press(&ctx, &mut sheet, Key::F2, Modifiers::NONE);
    frame(&ctx, &mut sheet, vec![]);
    assert_eq!(Grid::editing(&ctx, Grid::state_of("g")).map(|e| e.2), Some("1 mm".into()));
    frame(&ctx, &mut sheet, vec![Event::Text("x".into())]);
    let out = press(&ctx, &mut sheet, Key::Escape, Modifiers::NONE);
    assert!(out.edits.is_empty() && out.cancelled.is_some());
    assert!(Grid::editing(&ctx, Grid::state_of("g")).is_none());
    assert_eq!(sheet.cell(1, 1), "1 mm");
    // Typing on a read-only cell edits nothing.
    press(&ctx, &mut sheet, Key::ArrowRight, Modifiers::NONE);
    frame(&ctx, &mut sheet, vec![Event::Text("z".into())]);
    assert!(Grid::editing(&ctx, Grid::state_of("g")).is_none());
}

#[test]
fn ctrl_enter_writes_one_value_to_every_selected_cell() {
    let (ctx, mut sheet) = focused(3);
    // From Note (read-only) down and left: the range holds every column, the cursor is on Name.
    press(&ctx, &mut sheet, Key::End, Modifiers::NONE);
    press(&ctx, &mut sheet, Key::ArrowDown, Modifiers::SHIFT);
    press(&ctx, &mut sheet, Key::ArrowLeft, Modifiers::SHIFT);
    press(&ctx, &mut sheet, Key::ArrowLeft, Modifiers::SHIFT);
    frame(&ctx, &mut sheet, vec![Event::Text("same".into())]);
    assert_eq!(Grid::editing(&ctx, Grid::state_of("g")).map(|e| e.2), Some("same".into()));
    assert_eq!(selection(&ctx), (0, 1, 0, 2), "typing keeps the range");
    let out = press(&ctx, &mut sheet, Key::Enter, COMMAND);
    assert_eq!(out.gesture, Some(Gesture::CommitAll));
    assert_eq!(out.edits.len(), 4, "Name and Value of both rows, Note is read-only: {:?}", out.edits);
    assert!((0..2).all(|r| sheet.cell(r, 0) == "same" && sheet.cell(r, 1) == "same" && sheet.cell(r, 2) == format!("note{r}")));
}

#[test]
fn delete_clears_and_a_refused_cell_keeps_its_value_with_the_reason() {
    let (ctx, mut sheet) = focused(2);
    sheet.value_required = true;
    press(&ctx, &mut sheet, Key::ArrowDown, Modifiers::SHIFT);
    press(&ctx, &mut sheet, Key::ArrowRight, Modifiers::SHIFT);
    let out = press(&ctx, &mut sheet, Key::Delete, Modifiers::NONE);
    assert_eq!(out.gesture, Some(Gesture::Clear));
    assert_eq!(out.edits, [Edit::Clear { row: RowRef::Existing("r0".into()), col: 0 }, Edit::Clear { row: RowRef::Existing("r1".into()), col: 0 }]);
    assert_eq!(out.refused.as_deref(), Some("needs a value"));
    assert_eq!((sheet.cell(0, 0), sheet.cell(0, 1)), ("", "0 mm"));
    // A refused commit by Enter keeps the edit open.
    press(&ctx, &mut sheet, Key::ArrowUp, Modifiers::NONE);
    assert_eq!(selection(&ctx), (0, 0, 1, 1));
    press(&ctx, &mut sheet, Key::F2, Modifiers::NONE);
    // F2 selected the text: Delete in the box empties it.
    frame(&ctx, &mut sheet, key_with(Key::Delete, Modifiers::NONE));
    assert_eq!(Grid::editing(&ctx, Grid::state_of("g")).map(|e| e.2), Some(String::new()));
    let out = press(&ctx, &mut sheet, Key::Enter, Modifiers::NONE);
    assert_eq!(out.refused.as_deref(), Some("needs a value"));
    assert!(Grid::editing(&ctx, Grid::state_of("g")).is_some(), "the edit stays open to correct");
}

#[test]
fn refuse_reopens_a_keyboard_commit_with_its_text() {
    let (ctx, mut sheet) = focused(2);
    frame(&ctx, &mut sheet, vec![Event::Text("taken".into())]);
    let (out, _) = run(&ctx, &mut sheet, key_with(Key::Enter, Modifiers::NONE), Modifiers::NONE);
    assert_eq!(out.edits.len(), 1);
    // The app's command said no: nothing applied, the edit comes back.
    Grid::refuse(&ctx, Grid::state_of("g"), "there is already one");
    frame(&ctx, &mut sheet, vec![]);
    assert_eq!(Grid::editing(&ctx, Grid::state_of("g")), Some(("r0".into(), 0, "taken".into())));
    assert_eq!(sheet.cell(0, 0), "name0");
}

#[test]
fn only_the_visible_rows_of_ten_thousand_are_built() {
    let ctx = crate::test_support::context();
    let mut sheet = Sheet::new(10_000);
    frame(&ctx, &mut sheet, vec![]);
    let started = std::time::Instant::now();
    for _ in 0..10 {
        frame(&ctx, &mut sheet, vec![]);
    }
    let per_frame = started.elapsed().as_secs_f64() * 100.0;
    let visible = (400.0 / table_row_height()).ceil() as usize + 1;
    assert!(sheet.drawn <= visible * 3, "{} cells built for about {visible} visible rows", sheet.drawn);
    assert!(registry::rect_of("dialog:table:g:r9999").is_none());
    eprintln!("10,000-row grid: {per_frame:.2} ms a frame");
}

#[test]
fn every_cell_reads_as_column_and_row_with_its_value() {
    let ctx = crate::test_support::context();
    ctx.enable_accesskit();
    let mut sheet = Sheet::new(2);
    frame(&ctx, &mut sheet, vec![]);
    let (_, full) = run(&ctx, &mut sheet, vec![], Modifiers::NONE);
    let tree = full.platform_output.accesskit_update.expect("accesskit is on");
    let cell = tree.nodes.iter().map(|(_, n)| n).find(|n| n.label() == Some("Value, r1")).expect("a cell named by column and row");
    assert_eq!(cell.value(), Some("1 mm"));
    assert!(registry::rect_of("grid:g:r1:value").is_some(), "and a stable handle");
    assert!(registry::rect_of("dialog:table:g:r1:cell:Value").is_some(), "the table's own handles stay");
}
