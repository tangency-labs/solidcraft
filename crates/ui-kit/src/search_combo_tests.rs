use crate::test_support::*;
use crate::*;

use egui::Ui;
use solidcraft_ui_registry as registry;
use std::borrow::Cow;

#[test]
fn filtering_matches_anywhere_in_any_case() {
    let options = ["Body 1", "Shaft", "bracket body", "Ünterlegscheibe"];
    assert_eq!(combo_filter(&options[..], ""), [0, 1, 2, 3], "nothing typed lists all");
    assert_eq!(combo_filter(&options[..], "BODY"), [0, 2], "case does not matter, nor where");
    assert_eq!(combo_filter(&options[..], "  aft "), [1], "the typed text is trimmed");
    assert_eq!(combo_filter(&options[..], "ünter"), [3], "non-ASCII lowercases too");
    assert!(combo_filter(&options[..], "gear").is_empty());
    assert!(combo_matches("Anything", ""));
}

#[test]
fn up_and_down_move_the_highlight_within_the_list() {
    assert_eq!(combo_step(None, 5, 1, 0), Some(0), "Down starts at the first row");
    assert_eq!(combo_step(None, 5, 0, 1), Some(4), "Up starts at the last");
    assert_eq!(combo_step(None, 5, 0, 0), None);
    assert_eq!(combo_step(Some(1), 5, 2, 0), Some(3));
    assert_eq!(combo_step(Some(3), 5, 9, 0), Some(4), "stops at the end");
    assert_eq!(combo_step(Some(1), 5, 0, 4), Some(0), "stops at the top");
    assert_eq!(combo_step(Some(7), 5, 0, 0), Some(4), "a highlight past a shrunken list comes back into it");
    assert_eq!(combo_step(Some(2), 0, 1, 0), None, "nothing to highlight");
}

/// A list of ten thousand reads only the labels in view when it opens, and every label once
/// when searched; the keyboard picks, Esc closes and a click picks.
#[test]
fn search_combo_opens_a_long_list_lazily_and_drives_from_the_keyboard() {
    use std::cell::{Cell, RefCell};
    const COUNT: usize = 10_000;
    let ctx = context();
    let reads = Cell::new(0usize);
    let picked = RefCell::new(Vec::new());
    let source = ComboFn::new(COUNT, |i: usize| {
        reads.set(reads.get() + 1);
        Cow::Owned(format!("Item {i}"))
    });
    let draw = |ui: &mut Ui| {
        row(ui, "Pick", |ui| {
            let shown = SearchCombo::new("search_combo_test", "Item 0", &source).selected(Some(0)).handles("test:pick").show(ui);
            picked.borrow_mut().extend(shown.inner);
        });
    };
    let open = || {
        let at = registry::handle_at("test:pick").unwrap_or_else(|| panic!("no control"));
        frame(&ctx, pointer(at, true), draw);
        frame(&ctx, pointer(at, false), draw);
        frame(&ctx, vec![], draw);
    };
    let typing = |text: &str| {
        frame(&ctx, vec![egui::Event::Text(text.into())], draw);
        frame(&ctx, vec![], draw);
    };
    let press = |k: egui::Key| {
        frame(&ctx, key(k), draw);
        frame(&ctx, vec![], draw);
    };
    let is_open = || registry::handle_at("test:pick:search").is_some();

    frame(&ctx, vec![], draw);
    assert_eq!(reads.get(), 0, "closed, no label is read");
    open();
    assert!(is_open());
    assert!(ctx.memory(|m| m.has_focus(egui::Id::unique("search_combo_test").with("search_combo").with("search"))), "the box has the focus");
    assert!(registry::handle_at("test:pick:Item 0").is_some());
    assert!(registry::handle_at("test:pick:Item 9999").is_none(), "only the rows in view are drawn");
    assert!(reads.get() < 200, "opening read {} labels of {COUNT}", reads.get());

    // Down twice from the chosen row and Enter picks the third.
    press(egui::Key::ArrowDown);
    press(egui::Key::ArrowDown);
    press(egui::Key::Enter);
    assert_eq!(picked.take(), [2]);
    assert!(!is_open(), "a pick closes the popup");

    // Typing filters; Enter takes the first match; the next opening starts unfiltered.
    open();
    typing("item 999");
    assert!(registry::handle_at("test:pick:Item 999").is_some());
    assert!(registry::handle_at("test:pick:Item 0").is_none());
    typing("7");
    press(egui::Key::Enter);
    assert_eq!(picked.take(), [9997]);
    open();
    assert!(registry::handle_at("test:pick:Item 0").is_some(), "the filter was forgotten");

    // Up from the first row of a search wraps to nothing; it stays on the top row.
    typing("item 12");
    press(egui::Key::ArrowUp);
    press(egui::Key::Enter);
    assert_eq!(picked.take(), [12]);

    // Down past the rows in view scrolls the highlight into view.
    open();
    for _ in 0..30 {
        frame(&ctx, key(egui::Key::ArrowDown), draw);
    }
    frame(&ctx, vec![], draw);
    frame(&ctx, vec![], draw);
    assert!(registry::handle_at("test:pick:Item 30").is_some(), "the highlight is in view");
    press(egui::Key::Enter);
    assert_eq!(picked.take(), [30]);

    // No match: a note, and Enter keeps the popup; Esc closes without a pick.
    open();
    typing("gear");
    press(egui::Key::Enter);
    assert!(is_open() && picked.borrow().is_empty());
    press(egui::Key::Escape);
    assert!(!is_open() && picked.borrow().is_empty(), "Esc closes without a pick");

    // A click picks the row.
    open();
    let at = registry::handle_at("test:pick:Item 3").unwrap_or_else(|| panic!("no row"));
    frame(&ctx, pointer(at, true), draw);
    frame(&ctx, pointer(at, false), draw);
    frame(&ctx, vec![], draw);
    assert_eq!(picked.take(), [3]);
    assert!(!is_open());
}

/// Without handles, the control and its rows take the row label's names, as `dropdown` gives them.
#[test]
fn search_combo_names_its_rows_after_the_row_label_by_default() {
    let ctx = context();
    let options = vec!["Steel".to_owned(), "Aluminium".to_owned()];
    let draw = |ui: &mut Ui| {
        row(ui, "Material", |ui| {
            SearchCombo::new("search_combo_names", "Steel", &options).show(ui);
        });
    };
    frame(&ctx, vec![], draw);
    let at = registry::handle_at("dialog:Material").unwrap_or_else(|| panic!("no control"));
    frame(&ctx, pointer(at, true), draw);
    frame(&ctx, pointer(at, false), draw);
    frame(&ctx, vec![], draw);
    assert!(registry::handle_at("dialog:Material:Aluminium").is_some());
    assert!(registry::handle_at("dialog:Material:search").is_some());
}

/// Pictures of the control closed, open on ten thousand rows, searched, and moved with the
/// keyboard, in both themes: `SOLIDCRAFT_COMBO_EVIDENCE=<dir> cargo test -p solidcraft-ui-kit
/// --lib search_combo_pictures`. Without the variable it only checks that each draws.
#[test]
fn search_combo_pictures() {
    use std::collections::HashMap;
    let previous = solidcraft_theme::is_dark();
    let source = ComboFn::new(10_000, |i: usize| Cow::Owned(format!("Body {}", i + 1)));
    for dark in [false, true] {
        solidcraft_theme::set_dark(dark);
        let ctx = context();
        solidcraft_theme::apply(&ctx);
        let bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(420.0, 380.0));
        let mut textures = HashMap::new();
        let mut shot = |events: Vec<egui::Event>| {
            registry::clear_handles();
            let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(bounds), events, focused: true, ..Default::default() }, |ui| {
                ui.painter().rect_filled(bounds, 0.0, solidcraft_theme::Tokens::get().panel);
                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    ui.add_space(16.0);
                    ui.vertical(|ui| {
                        ui.set_width(360.0);
                        row(ui, "Target body", |ui| {
                            SearchCombo::new("combo_pictures", "Body 1", &source).selected(Some(0)).handles("pictures:body").show(ui);
                        });
                    });
                });
            });
            textures_update(&mut textures, &output.textures_delta);
            output.textures_delta.clear();
            raster(&ctx, output.shapes, output.pixels_per_point, bounds, &textures)
        };
        let mut pictures = vec![("closed", shot(vec![]))];
        let at = registry::handle_at("pictures:body").unwrap_or_else(|| panic!("no control"));
        shot(pointer(at, true));
        shot(pointer(at, false));
        shot(vec![]);
        pictures.push(("open-10000", shot(vec![])));
        for _ in 0..3 {
            shot(key(egui::Key::ArrowDown));
        }
        shot(vec![]);
        pictures.push(("keyboard", shot(vec![])));
        shot(vec![egui::Event::Text("body 99".into())]);
        shot(vec![]);
        pictures.push(("searched", shot(vec![])));
        shot(vec![egui::Event::Text("x".into())]);
        shot(vec![]);
        pictures.push(("no-match", shot(vec![])));
        for (name, image) in pictures {
            assert!(image.pixels().any(|p| p.0[3] != 0), "{name}");
            if let Ok(dir) = std::env::var("SOLIDCRAFT_COMBO_EVIDENCE") {
                let _ = std::fs::create_dir_all(&dir);
                let theme = if dark { "dark" } else { "light" };
                let _ = image.save(std::path::Path::new(&dir).join(format!("search-combo-{name}-{theme}.png")));
            }
        }
    }
    solidcraft_theme::set_dark(previous);
}
