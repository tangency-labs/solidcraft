use crate::test_support::*;
use crate::*;

fn key(key: egui::Key, shift: bool) -> Vec<egui::Event> {
    let modifiers = if shift { egui::Modifiers::SHIFT } else { egui::Modifiers::NONE };
    vec![
        egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers },
        egui::Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers },
    ]
}

fn typed(text: &str) -> Vec<egui::Event> {
    vec![egui::Event::Text(text.to_owned())]
}

/// A form of a text box, a value (expression) box, a number box and a table cell, Tab-ordered.
struct Form {
    ctx: egui::Context,
    text: String,
    value: String,
    number: f64,
    cell: String,
    rects: [egui::Rect; 4],
}

impl Form {
    fn new() -> Self {
        let ctx = context();
        install_keyboard_focus(&ctx);
        let mut form =
            Self { ctx, text: "Bracket".into(), value: "12 mm".into(), number: 40.0, cell: "width".into(), rects: [egui::Rect::NOTHING; 4] };
        form.frame(Vec::new());
        form
    }

    /// One frame with `events`, then two quiet ones for the focus to settle.
    fn frame(&mut self, events: Vec<egui::Event>) {
        for events in [events, Vec::new(), Vec::new()] {
            let input = egui::RawInput {
                events,
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0))),
                ..Default::default()
            };
            let (text, value, number, cell, rects) = (&mut self.text, &mut self.value, &mut self.number, &mut self.cell, &mut self.rects);
            raw_frame(&self.ctx, input, |ui| {
                ui.set_width(280.0);
                scope(ui, |ui| {
                    rects[0] = text_field_reserving(ui, text, "Name", 0.0).1.rect;
                    rects[1] = text_field_reserving(ui, value, "Value", 0.0).1.rect;
                    rects[2] = number_field(ui, number, 0.0..=100.0, 1.0, "").rect;
                    rects[3] = cell_editor(ui, egui::Id::unique("form cell"), cell, "", false).rect;
                });
            });
        }
    }
}

/// Tab and Shift+Tab into a text box, a value box, a number box and a table cell select their
/// text, so typing replaces it; a click puts the caret where it lands, as before.
#[test]
fn tab_into_a_text_or_number_box_selects_its_text_and_a_click_does_not() {
    let mut f = Form::new();
    f.frame(key(egui::Key::Tab, false));
    f.frame(typed("Plate"));
    assert_eq!(f.text, "Plate", "Tab into the text box selected it, typing replaced it");
    f.frame(key(egui::Key::Tab, false));
    f.frame(typed("7 mm"));
    assert_eq!(f.value, "7 mm", "Tab into the value box selected it");
    f.frame(key(egui::Key::Tab, false));
    f.frame(typed("25"));
    f.frame(key(egui::Key::Tab, false));
    assert_eq!(f.number, 25.0, "Tab into the number box selected it");
    f.frame(typed("depth"));
    assert_eq!(f.cell, "depth", "Tab into the table cell selected it");
    f.frame(key(egui::Key::Tab, true));
    f.frame(typed("30"));
    f.frame(key(egui::Key::Tab, true));
    assert_eq!(f.number, 30.0, "Shift+Tab back into the number box selected it too");
    // Shift+Tab out of a text box into the one before it.
    let mut f = Form::new();
    f.frame(key(egui::Key::Tab, false));
    f.frame(key(egui::Key::Tab, false));
    f.frame(key(egui::Key::Tab, true));
    f.frame(typed("Plate"));
    assert_eq!((f.text.as_str(), f.value.as_str()), ("Plate", "12 mm"), "Shift+Tab back into the text box selected it");

    // A click at the end of the text box puts the caret there: typing adds to the text.
    let mut f = Form::new();
    let at = egui::pos2(f.rects[1].right() - 4.0, f.rects[1].center().y);
    f.frame(pointer(at, true));
    f.frame(pointer(at, false));
    f.frame(typed("0"));
    assert_eq!(f.value, "12 mm0", "a click keeps the caret where it lands");
}

/// Enter or Shift+Enter in a table cell reports the move; a held key, a Ctrl+Enter or another
/// cell's key does not.
#[test]
fn a_cell_reads_enter_down_shift_enter_up_and_tab_along() {
    let ctx = context();
    let mut seen = Vec::new();
    for (events, focused) in [
        (key(egui::Key::Enter, false), true),
        (key(egui::Key::Enter, true), true),
        (key(egui::Key::Tab, false), true),
        (key(egui::Key::Tab, true), true),
        (key(egui::Key::Enter, false), false),
        (
            vec![egui::Event::Key { key: egui::Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::COMMAND }],
            true,
        ),
        (vec![egui::Event::Key { key: egui::Key::Enter, physical_key: None, pressed: true, repeat: true, modifiers: egui::Modifiers::NONE }], true),
    ] {
        raw_frame(&ctx, egui::RawInput { events, ..Default::default() }, |ui| seen.push(cell_key(ui, focused)));
    }
    assert_eq!(seen, [Some(CellKey::Down), Some(CellKey::Up), Some(CellKey::Next), Some(CellKey::Previous), None, None, None]);
}
