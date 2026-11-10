use crate::ui_kit::test_support::*;
use crate::ui_kit::*;

use egui::Ui;
use solidcraft_ui_registry as registry;

#[test]
fn combo_backing_zero_preserves_stock_shapes_and_semantics() {
    let fixture = |stock: bool| {
        let ctx = crate::ui_kit::test_support::context();
        let mut id = egui::Id::NULL;
        let mut semantics = String::new();
        let mut output = egui::FullOutput::default();
        for pass in 0..4 {
            if pass > 0 {
                egui::Popup::open_id(&ctx, id.with("popup"));
            }
            output = crate::ui_kit::test_support::raw_frame(&ctx, egui::RawInput::default(), |ui| {
                let salt = egui::IdSalt::new("stock combo");
                let widget = ui.make_persistent_id(salt);
                let combo = egui::ComboBox::from_id_salt("stock combo").selected_text("One");
                let mut body = |ui: &mut Ui| {
                    let response = ui.button("Two");
                    semantics = format!("{:?}", (ui.scope_id(), ui.max_rect(), ui.clip_rect(), response.id, response.rect));
                };
                let response = if stock { combo.show_ui(ui, &mut body) } else { container::show_combo(ui, widget, combo, &mut body) };
                id = response.response.id;
                assert_eq!(widget, id);
                semantics.push_str(&format!("{:?}", (id, response.response.rect, response.inner)));
            });
        }
        let mut shapes = Vec::new();
        for clipped in &output.shapes {
            crate::ui_kit::test_support::each_shape(&clipped.shape, &mut |shape| shapes.push(format!("{:?} {shape:?}", clipped.clip_rect)));
        }
        (semantics, shapes)
    };
    assert_eq!(fixture(true), fixture(false), "zero adds no shapes or slots and keeps native IDs, geometry and clip");
}

#[test]
fn every_text_input_centres_its_galley_for_mono_and_proportional_fonts() {
    for case in ["expression", "plain", "cell", "invalid", "library", "placement", "password", "multiline", "area", "message", "search"] {
        let ctx = crate::ui_kit::test_support::context();
        let mut value = "Centred".to_owned();
        let mut rect = egui::Rect::NOTHING;
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.set_width(280.0);
            scope(ui, |ui| {
                let id = ui.scope_id().with(case);
                rect = match case {
                    "expression" | "plain" => super::editor(ui, &mut value, "", case == "expression").2.rect,
                    "search" => search_field(ui, id, &mut value, "Search").rect,
                    "cell" | "invalid" => cell_editor(ui, id, &mut value, "", case == "invalid").rect,
                    "library" => library_cell(ui, id, &mut value, "", |_| {}).rect,
                    "placement" => placement_field(ui, id, &mut value, "").rect,
                    "password" => password_field(ui, id, &mut value, "").rect,
                    "multiline" => multiline_text_field(ui, "Text", id, &mut value).rect,
                    "area" => text_area(ui, "Text", id, &mut value, 3, 100).rect,
                    _ => message_field(ui, id, &mut value, "").rect,
                };
            });
        });
        output.textures_delta.clear();
        let galley = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.job.text == value || (case == "password" && !text.galley.is_empty()) => Some(text),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{case}: missing text"));
        let center = galley.pos.y + galley.galley.rect.center().y;
        assert!((center - rect.center().y).abs() < 0.01, "{case}: galley centre {center} differs from field {rect:?}");
        if case == "search" {
            assert!(galley.pos.x >= rect.left() + solidcraft_theme::Tokens::get().space_24, "search text leaves room for its icon");
        }
        if case == "placement" {
            let font = egui::FontId::proportional(solidcraft_theme::Tokens::get().font_body);
            let native = ctx.fonts_mut(|fonts| fonts.row_height(&font))
                + ctx.style_of(egui::Theme::Light).spacing.extra_text_line_spacing
                + solidcraft_theme::Tokens::get().space_4;
            assert_eq!(rect.height(), native, "canvas editor retains its native hit target");
        } else if !matches!(case, "multiline" | "area" | "message") {
            assert_eq!(rect.height(), solidcraft_theme::Tokens::get().control_height, "{case}");
        }
    }
}

#[test]
fn tab_visits_expression_fields_in_row_order() {
    let ctx = crate::ui_kit::test_support::context();
    let mut first = "1".to_string();
    let mut second = String::new();
    let mut draw = |ui: &mut Ui| {
        row(ui, "First value", |ui| {
            field(ui, &mut first);
        });
        row(ui, "Second value", |ui| {
            field(ui, &mut second);
        });
    };
    frame(&ctx, vec![], &mut draw);
    let first_pos = registry::handle_at("dialog:First value").unwrap();
    frame(&ctx, pointer(first_pos, true), &mut draw);
    frame(&ctx, pointer(first_pos, false), &mut draw);
    frame(&ctx, key(egui::Key::Tab), &mut draw);
    frame(&ctx, vec![egui::Event::Text("2".to_string())], &mut draw);
    assert_eq!(first, "1");
    assert_eq!(second, "2");
}

#[test]
fn multiline_text_retains_focus_id_and_three_line_geometry() {
    let ctx = crate::ui_kit::test_support::context();
    let id = egui::Id::unique("multiline_test");
    let mut value = "First\nSecond".to_string();
    let mut draw = |ui: &mut Ui| {
        multiline_text_field(ui, "Text", id, &mut value);
    };
    frame(&ctx, vec![], &mut draw);
    let row = registry::rect_of("dialog:row:Text").unwrap();
    assert_eq!(row.height(), 3.0 * solidcraft_theme::Tokens::get().control_height);
    let at = registry::handle_at("dialog:Text").unwrap();
    frame(&ctx, pointer(at, true), &mut draw);
    frame(&ctx, pointer(at, false), &mut draw);
    assert!(ctx.memory(|m| m.has_focus(id)));
}

#[test]
fn searchable_dropdown_filters_selects_and_accepts_custom_values() {
    for custom in [false, true] {
        let ctx = crate::ui_kit::test_support::context();
        let mut filter = if custom { "/fonts/face.ttf".into() } else { "tw".into() };
        let mut chosen = None;
        let options = [
            SearchOption { value: "one", label: "One", handle: Some("search:one") },
            SearchOption { value: "two", label: "Two", handle: Some("search:two") },
        ];
        let mut draw = |ui: &mut Ui| {
            row(ui, "Font", |ui| {
                if let Some(value) =
                    searchable_dropdown(ui, "search_test", "One", &mut filter, &options, custom.then_some("Use this font file"), None)
                {
                    chosen = Some(value);
                }
            });
        };
        frame(&ctx, vec![], &mut draw);
        let at = registry::handle_at("dialog:Font").unwrap();
        frame(&ctx, pointer(at, true), &mut draw);
        frame(&ctx, pointer(at, false), &mut draw);
        frame(&ctx, vec![], &mut draw);
        assert!(registry::handle_at("dialog:Font search").is_some());
        assert!(registry::handle_at("search:one").is_none(), "filter hides unmatched entries");
        let pick = registry::handle_at(if custom { "dialog:Font file" } else { "search:two" }).unwrap();
        frame(&ctx, pointer(pick, true), &mut draw);
        frame(&ctx, pointer(pick, false), &mut draw);
        assert_eq!(chosen.as_deref(), Some(if custom { "/fonts/face.ttf" } else { "two" }));
    }
}

#[test]
fn search_picker_filters_focuses_picks_and_takes_custom_values() {
    let ctx = crate::ui_kit::test_support::context();
    let picked = std::cell::RefCell::new(Vec::new());
    let custom = std::cell::Cell::new(true);
    let enabled = std::cell::Cell::new(true);
    let choices = [
        SearchChoice { value: "a/one", label: "One", tip: "The first model", group: None },
        SearchChoice { value: "b/two", label: "Two", tip: "The second model", group: Some("Group") },
        SearchChoice { value: "b/three", label: "Three", tip: "", group: Some("Group") },
    ];
    let draw = |ui: &mut Ui| {
        row(ui, "Model", |ui| {
            let shown = search_picker(
                ui,
                SearchPicker {
                    id: "search_picker_test",
                    handles: "test:model",
                    current: "One",
                    current_value: "a/one",
                    hint: "Search models…",
                    choices: &choices,
                    custom: custom.get(),
                    note: Some("Listing models…"),
                    open_group: None,
                    enabled: enabled.get(),
                },
            );
            picked.borrow_mut().extend(shown.inner);
        });
    };
    let open = || {
        let at = registry::handle_at("test:model").unwrap();
        frame(&ctx, pointer(at, true), draw);
        frame(&ctx, pointer(at, false), draw);
        frame(&ctx, vec![], draw);
    };
    let click = |name: &str| {
        let at = registry::handle_at(name).unwrap_or_else(|| panic!("no {name}"));
        frame(&ctx, pointer(at, true), draw);
        frame(&ctx, pointer(at, false), draw);
        frame(&ctx, vec![], draw);
    };
    let typing = |text: &str| {
        frame(&ctx, vec![egui::Event::Text(text.into())], draw);
        frame(&ctx, vec![], draw);
    };
    let is_open = || registry::handle_at("test:model:search").is_some();
    frame(&ctx, vec![], draw);
    assert!(!is_open() && registry::handle_at("test:model:a/one").is_none(), "closed: only the control");

    // Opening focuses the box: what is typed filters over labels, tips and values.
    open();
    assert!(is_open());
    assert!(ctx.memory(|m| m.has_focus(egui::Id::unique("search_picker_test").with("search"))), "the box has the focus");
    for row in ["a/one", "b/two", "b/three"] {
        assert!(registry::handle_at(&format!("test:model:{row}")).is_some(), "{row}");
    }
    assert!(registry::handle_at("test:model:custom").is_none(), "nothing typed, nothing to use");
    typing("SECOND");
    assert!(registry::handle_at("test:model:b/two").is_some(), "a tip is searched, whatever the case");
    assert!(registry::handle_at("test:model:a/one").is_none() && registry::handle_at("test:model:b/three").is_none());
    assert!(registry::handle_at("test:model:custom").is_some(), "a substring is no listed value");

    // A click picks the row, closes the popup and forgets the text.
    click("test:model:b/two");
    assert_eq!(picked.take(), [Picked::Listed("b/two".into())]);
    assert!(!is_open());
    open();
    assert!(registry::handle_at("test:model:a/one").is_some(), "the next opening starts unfiltered");

    // A value exactly as listed needs no custom row; a text nothing matches gets one.
    typing("b/three");
    assert!(registry::handle_at("test:model:custom").is_none());
    assert!(registry::handle_at("test:model:b/three").is_some());
    frame(&ctx, key(egui::Key::Escape), draw);
    frame(&ctx, vec![], draw);
    assert!(!is_open() && picked.borrow().is_empty(), "Esc closes without a pick");
    open();
    typing("  vendor/model:free ");
    assert!(registry::handle_at("test:model:a/one").is_none());
    click("test:model:custom");
    assert_eq!(picked.take(), [Picked::Custom("vendor/model:free".into())], "trimmed");
    assert!(!is_open());

    // Enter picks the first row shown; with no match, the typed text.
    open();
    typing("t");
    frame(&ctx, key(egui::Key::Enter), draw);
    frame(&ctx, vec![], draw);
    assert_eq!(picked.take(), [Picked::Listed("a/one".into())], "the first row with a t");
    assert!(!is_open());
    open();
    typing("b/");
    typing("three");
    frame(&ctx, key(egui::Key::Enter), draw);
    frame(&ctx, vec![], draw);
    assert_eq!(picked.take(), [Picked::Listed("b/three".into())]);
    open();
    typing("nothing");
    frame(&ctx, key(egui::Key::Enter), draw);
    frame(&ctx, vec![], draw);
    assert_eq!(picked.take(), [Picked::Custom("nothing".into())]);

    // Enter on an empty box leaves the choice as it is.
    open();
    frame(&ctx, key(egui::Key::Enter), draw);
    frame(&ctx, vec![], draw);
    assert!(picked.borrow().is_empty() && !is_open());

    // Without custom values, a miss offers nothing and Enter keeps the box.
    custom.set(false);
    open();
    typing("nothing");
    assert!(registry::handle_at("test:model:custom").is_none());
    frame(&ctx, key(egui::Key::Enter), draw);
    frame(&ctx, vec![], draw);
    assert!(picked.borrow().is_empty() && is_open());
    typing("!");
    frame(&ctx, key(egui::Key::Escape), draw);
    frame(&ctx, vec![], draw);
    assert!(!is_open());

    // A disabled control does not open.
    enabled.set(false);
    open();
    assert!(!is_open());
}

/// A long list costs only the rows in view, and a long label neither widens the popup past the
/// window nor reaches past its edge.
/// A popup can't grow once it is open, so what appears in it later must fit in the room it opened
/// with: a custom row above a short list, a list that was empty, a note, and a list again after a
/// search that matched nothing.
#[test]
fn search_picker_keeps_every_row_reachable_as_its_contents_change() {
    use std::cell::{Cell, RefCell};
    let ctx = crate::ui_kit::test_support::context();
    let values = RefCell::new(vec!["a/one", "b/two"]);
    let note: Cell<Option<&str>> = Cell::new(None);
    let picked = RefCell::new(Vec::new());
    let frame = |events: Vec<egui::Event>| {
        registry::clear_handles();
        let raw =
            egui::RawInput { events, screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 700.0))), ..Default::default() };
        ctx.run_ui(raw, |ui| {
            ui.allocate_ui(egui::vec2(160.0, 40.0), |ui| {
                crate::ui_kit::scope(ui, |ui| {
                    let values = values.borrow();
                    let choices: Vec<SearchChoice<'_>> =
                        values.iter().map(|value| SearchChoice { value, label: value, tip: "", group: None }).collect();
                    let result = search_picker(
                        ui,
                        SearchPicker {
                            id: "search_picker_room",
                            handles: "room",
                            current: "b/two",
                            current_value: "b/two",
                            hint: "Search",
                            choices: &choices,
                            custom: true,
                            note: note.get(),
                            open_group: None,
                            enabled: true,
                        },
                    );
                    if let Some(pick) = result.inner {
                        picked.borrow_mut().push(pick);
                    }
                });
            });
        })
        .textures_delta
        .clear();
    };
    let pointer = |at: egui::Pos2, pressed: bool| {
        vec![
            egui::Event::PointerMoved(at),
            egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE },
        ]
    };
    let click = |name: &str| {
        let at = registry::handle_at(name).unwrap_or_else(|| panic!("no {name}"));
        frame(pointer(at, true));
        frame(pointer(at, false));
        frame(vec![]);
        frame(vec![]);
    };
    let typing = |text: &str| {
        frame(vec![egui::Event::Text(text.into())]);
        frame(vec![]);
        frame(vec![]);
    };
    let erase = |times: usize| {
        for _ in 0..times {
            frame(vec![egui::Event::Key {
                key: egui::Key::Backspace,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }]);
            frame(vec![]);
        }
        frame(vec![]);
    };
    frame(vec![]);
    frame(vec![]);

    // The custom row appears above the two rows: the row under it can still be clicked.
    click("room");
    typing("two");
    assert!(registry::handle_at("room:custom").is_some());
    click("room:b/two");
    assert_eq!(picked.take(), [Picked::Listed("b/two".into())]);

    // A search that matches nothing leaves no list; taking it back brings the rows, in reach.
    click("room");
    typing("zzz");
    assert!(registry::handle_at("room:a/one").is_none());
    frame(vec![]);
    frame(vec![]);
    erase(3);
    click("room:b/two");
    assert_eq!(picked.take(), [Picked::Listed("b/two".into())]);

    // A list that arrives while it is open, with a long note, stays in reach; the custom row too.
    values.borrow_mut().clear();
    note.set(Some("Listing models…"));
    frame(vec![]);
    click("room");
    frame(vec![]);
    values.borrow_mut().extend(["a/one", "b/two", "c/three", "d/four"]);
    note.set(Some("A long reason, repeated until it wraps over a few lines of the popup, repeated until it wraps over a few lines of the popup."));
    frame(vec![]);
    frame(vec![]);
    typing("four");
    click("room:d/four");
    assert_eq!(picked.take(), [Picked::Listed("d/four".into())]);
    click("room");
    typing("nothing");
    click("room:custom");
    assert_eq!(picked.take(), [Picked::Custom("nothing".into())]);
}

#[test]
fn search_picker_lays_out_the_rows_in_view_inside_a_narrow_window() {
    let ctx = crate::ui_kit::test_support::context();
    let values: Vec<String> = (0..2000).map(|n| format!("vendor/a-model-with-a-rather-long-name-{n:04}-and-then-some-more-of-it-and-more")).collect();
    let choices: Vec<SearchChoice<'_>> = values.iter().map(|v| SearchChoice { value: v, label: v, tip: "", group: None }).collect();
    let draw = |ui: &mut Ui| {
        ui.allocate_ui(egui::vec2(120.0, 28.0), |ui| {
            search_picker(
                ui,
                SearchPicker {
                    id: "search_picker_long",
                    handles: "test:long",
                    current: "One",
                    current_value: "",
                    hint: "",
                    choices: &choices,
                    custom: false,
                    note: None,
                    open_group: None,
                    enabled: true,
                },
            );
        });
    };
    frame(&ctx, vec![], draw);
    let at = registry::handle_at("test:long").unwrap();
    frame(&ctx, pointer(at, true), draw);
    frame(&ctx, pointer(at, false), draw);
    frame(&ctx, vec![], draw);
    frame(&ctx, vec![], draw);
    let rows = registry::handles("test:long:vendor/");
    assert!(!rows.is_empty() && rows.len() < 30, "{} rows laid out", rows.len());
    let row = registry::rect_of("test:long:vendor/a-model-with-a-rather-long-name-0000-and-then-some-more-of-it-and-more").unwrap();
    assert!(row.left() >= 0.0 && row.right() <= 320.0, "{row:?} reaches past the window");
    let search = registry::rect_of("test:long:search").unwrap();
    assert!(search.left() >= 0.0 && search.right() <= 320.0, "{search:?} reaches past the window");
}

/// Several groups fold under their headers: at first only the current value's group and the one
/// asked for are open; a header opens or folds its group; a typed search opens the groups with a
/// match but one folded by hand; Enter takes the first row shown; a fresh opening forgets the folds.
#[test]
fn search_picker_folds_groups_under_their_headers() {
    let ctx = crate::ui_kit::test_support::context();
    let picked = std::cell::RefCell::new(Vec::new());
    let open_group = std::cell::Cell::new(None::<&'static str>);
    let current = std::cell::Cell::new("b1");
    let choices = [
        SearchChoice { value: "a1", label: "Alpha one", tip: "", group: Some("A") },
        SearchChoice { value: "b1", label: "Beta one", tip: "", group: Some("B") },
        SearchChoice { value: "a2", label: "Alpha two", tip: "", group: Some("A") },
        SearchChoice { value: "c1", label: "Gamma one", tip: "", group: Some("C") },
    ];
    let draw = |ui: &mut Ui| {
        row(ui, "Model", |ui| {
            let shown = search_picker(
                ui,
                SearchPicker {
                    id: "search_picker_groups",
                    handles: "grp",
                    current: "Beta one",
                    current_value: current.get(),
                    hint: "",
                    choices: &choices,
                    custom: false,
                    note: None,
                    open_group: open_group.get(),
                    enabled: true,
                },
            );
            picked.borrow_mut().extend(shown.inner);
        });
    };
    let click = |name: &str| {
        let at = registry::handle_at(name).unwrap_or_else(|| panic!("no {name}"));
        frame(&ctx, pointer(at, true), draw);
        frame(&ctx, pointer(at, false), draw);
        frame(&ctx, vec![], draw);
    };
    let typing = |text: &str| {
        frame(&ctx, vec![egui::Event::Text(text.into())], draw);
        frame(&ctx, vec![], draw);
    };
    let has = |name: &str| registry::handle_at(name).is_some();
    frame(&ctx, vec![], draw);
    click("grp");
    assert!(has("grp:group:A") && has("grp:group:B") && has("grp:group:C"), "a header per group");
    assert!(has("grp:b1") && !has("grp:a1") && !has("grp:c1"), "only the current value's group is open");
    click("grp:group:A");
    assert!(has("grp:a1") && has("grp:a2"), "a header opens its group");
    click("grp:group:A");
    assert!(!has("grp:a1"), "and folds it again");
    typing("one");
    assert!(!has("grp:a1"), "a fold made by hand stays while searching");
    assert!(has("grp:b1") && has("grp:c1"), "groups with a match open");
    frame(&ctx, key(egui::Key::Enter), draw);
    frame(&ctx, vec![], draw);
    assert_eq!(picked.take(), [Picked::Listed("b1".into())], "Enter takes the first row shown, not one folded away");
    assert!(!has("grp:search"));

    open_group.set(Some("C"));
    click("grp");
    assert!(!has("grp:a1") && has("grp:b1") && has("grp:c1"), "C opens as asked, B as the current value's, A folds");
    frame(&ctx, key(egui::Key::Escape), draw);
    frame(&ctx, vec![], draw);

    open_group.set(None);
    current.set("");
    click("grp");
    assert!(has("grp:a1") && has("grp:b1") && has("grp:c1"), "nothing to prefer: every group starts open");
    frame(&ctx, key(egui::Key::Escape), draw);
    frame(&ctx, vec![], draw);
}

#[test]
fn preference_dropdown_keeps_disabled_choices_inert_and_closes_after_selection() {
    let ctx = crate::ui_kit::test_support::context();
    let picked = std::cell::Cell::new(1);
    let mut draw = |ui: &mut Ui| {
        row(ui, "Samples", |ui| {
            dropdown_menu(ui, "samples", &picked.get().to_string(), |ui| {
                for (value, enabled) in [(1, true), (4, false), (8, true)] {
                    let response =
                        ui.add_enabled_ui(enabled, |ui| list_picker(ui, &value.to_string(), &format!("{value}×"), picked.get() == value)).inner;
                    if response.clicked() {
                        picked.set(value);
                    }
                }
            });
        });
    };
    frame(&ctx, vec![], &mut draw);
    let at = registry::handle_at("dialog:Samples").unwrap();
    frame(&ctx, pointer(at, true), &mut draw);
    frame(&ctx, pointer(at, false), &mut draw);
    let disabled = registry::handle_at("dialog:list:4").unwrap();
    frame(&ctx, pointer(disabled, true), &mut draw);
    frame(&ctx, pointer(disabled, false), &mut draw);
    frame(&ctx, vec![], &mut draw);
    assert_eq!(picked.get(), 1, "a disabled choice must not change the value");
    // Reopen if the native popup closed on the inside click.
    if registry::handle_at("dialog:list:8").is_none() {
        let at = registry::handle_at("dialog:Samples").unwrap();
        frame(&ctx, pointer(at, true), &mut draw);
        frame(&ctx, pointer(at, false), &mut draw);
        frame(&ctx, vec![], &mut draw);
    }
    let enabled = registry::handle_at("dialog:list:8").unwrap();
    frame(&ctx, pointer(enabled, true), &mut draw);
    frame(&ctx, pointer(enabled, false), &mut draw);
    frame(&ctx, vec![], &mut draw);
    assert!(registry::handle_at("dialog:list:8").is_none());
    assert_eq!(picked.get(), 8);
}

#[test]
fn dropdown_popups_own_escape_and_return_focus_without_canceling_the_window() {
    use std::cell::Cell;
    let previous = solidcraft_theme::is_dark();
    for dark in [false, true] {
        solidcraft_theme::set_dark(dark);
        for kind in ["plain", "icons", "compact", "editable"] {
            let ctx = crate::ui_kit::test_support::context();
            let action = Cell::new(Action::None);
            let trigger = Cell::new(None);
            let selected = Cell::new(0);
            let mut draw = |ui: &mut Ui| {
                let ctx = ui.ctx().clone();
                action.set(
                    Window::new(egui::Id::unique(("dropdown_escape", kind)), "settings", "Preferences")
                        .cards(1.0)
                        .fields()
                        .actions("OK", "Cancel", None)
                        .show(&ctx, true, |ui| {
                            // An earlier, closed dropdown must not steal this popup's Escape or focus.
                            row(ui, "Units", |ui| {
                                dropdown(ui, "units", &mut 0, &["mm", "in"]);
                            });
                            row(ui, "Visual style", |ui| {
                                let options = crate::navigation_slots::VISUAL_STYLES.map(|(_, label)| label);
                                let icons = crate::navigation_slots::VISUAL_STYLES
                                    .iter()
                                    .enumerate()
                                    .map(|(index, (_, label))| (*label, crate::navigation_slots::visual_style_icon(index as u8)))
                                    .collect::<Vec<_>>();
                                let mut choice = selected.get();
                                let choices = |ui: &mut Ui| {
                                    for (index, option) in options.iter().enumerate() {
                                        let response = ui.selectable_label(selected.get() == index, *option);
                                        registry::publish_target(&format!("dialog:Visual style:{option}"), response.rect);
                                        if response.clicked() {
                                            selected.set(index);
                                        }
                                    }
                                };
                                let response = match kind {
                                    "plain" => dropdown(ui, "styles", &mut choice, &options),
                                    "icons" => dropdown_icons(ui, "styles", &mut choice, &icons),
                                    "compact" => compact_dropdown_menu(ui, "styles", options[selected.get()], choices),
                                    _ => dropdown_menu_with_close_behavior(
                                        ui,
                                        "styles",
                                        options[selected.get()],
                                        egui::PopupCloseBehavior::CloseOnClickOutside,
                                        choices,
                                    ),
                                };
                                trigger.set(Some(response.id));
                                if matches!(kind, "plain" | "icons") {
                                    selected.set(choice);
                                }
                            });
                        })
                        .1,
                );
            };
            for _ in 0..3 {
                frame(&ctx, vec![], &mut draw);
            }
            let at = registry::handle_at("dialog:Visual style").unwrap();
            frame(&ctx, pointer(at, true), &mut draw);
            frame(&ctx, pointer(at, false), &mut draw);
            frame(&ctx, vec![], &mut draw);
            assert!(registry::handle_at("dialog:Visual style:Wireframe").is_some(), "{kind}: popup opens");
            frame(&ctx, key(egui::Key::Escape), &mut draw);
            assert_eq!(action.get(), Action::None, "{kind}: first Escape belongs to the popup");
            assert!(ctx.memory(|m| m.has_focus(trigger.get().unwrap())), "{kind}: Escape refocuses its own trigger");
            frame(&ctx, vec![], &mut draw);
            assert!(registry::handle_at("dialog:Visual style:Wireframe").is_none());
            frame(&ctx, key(egui::Key::Enter), &mut draw);
            assert_eq!(action.get(), Action::None, "{kind}: Enter reopens rather than submitting");
            frame(&ctx, vec![], &mut draw);
            assert!(registry::handle_at("dialog:Visual style:Wireframe").is_some(), "{kind}: focused trigger reopens on Enter");
            frame(&ctx, key(egui::Key::Escape), &mut draw);
            assert_eq!(action.get(), Action::None);
            frame(&ctx, vec![], &mut draw);
            assert!(registry::handle_at("dialog:Visual style:Wireframe").is_none());
            frame(&ctx, key(egui::Key::Escape), &mut draw);
            assert_eq!(action.get(), Action::Cancel, "{kind}: Escape without a popup cancels normally");
            assert_eq!(selected.get(), 0, "dismissal does not select an option");
        }
    }
    solidcraft_theme::set_dark(previous);
}

#[test]
fn secret_field_retains_focus_and_masks_paint_and_accessibility_events() {
    let ctx = crate::ui_kit::test_support::context();
    let id = egui::Id::unique("secret_test");
    let mut secret = String::new();
    let mut draw = |ui: &mut Ui| {
        row(ui, "API key", |ui| {
            password_field(ui, id, &mut secret, "Key").request_focus();
        });
    };
    frame(&ctx, vec![], &mut draw);
    assert!(ctx.memory(|m| m.has_focus(id)));
    let mut output = ctx
        .run_ui(egui::RawInput { events: vec![egui::Event::Text("opaque-secret".into())], focused: true, ..Default::default() }, |ui| {
            scope(ui, &mut draw)
        });
    assert!(!format!("{:?}", output.platform_output.events).contains("opaque-secret"));
    assert!(!format!("{:?}", output.shapes).contains("opaque-secret"));
    output.textures_delta.clear();
    assert_eq!(secret, "opaque-secret");
}

#[test]
fn growing_message_field_retains_its_id_and_does_not_consume_enter() {
    let ctx = crate::ui_kit::test_support::context();
    let id = egui::Id::unique("message_test");
    let mut message = String::new();
    let mut enter = false;
    let mut draw = |ui: &mut Ui| {
        let response = message_field(ui, id, &mut message, "Message");
        response.request_focus();
        enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
    };
    frame(&ctx, vec![], &mut draw);
    frame(&ctx, vec![egui::Event::Text("First\nSecond".into())], &mut draw);
    frame(&ctx, key(egui::Key::Enter), &mut draw);
    assert!(enter && ctx.memory(|m| m.has_focus(id)));
    assert_eq!(message, "First\nSecond\n");
}

/// The value fields on the canvas keep their width for a number and grow with an expression typed
/// into them (parameter names, a formula), up to three times that width, also in the area they
/// float in on the canvas, whose space is last frame's size.
#[test]
fn canvas_value_fields_grow_with_an_expression_up_to_three_times_their_width() {
    for case in ["canvas", "placement"] {
        let ctx = crate::ui_kit::test_support::context();
        let width = |value: &str| {
            let mut value = value.to_owned();
            let mut rect = egui::Rect::NOTHING;
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                egui::Area::new(egui::Id::unique("grow")).fixed_pos(egui::pos2(10.0, 10.0)).show(ui.ctx(), |ui| {
                    ui.horizontal(|ui| {
                        let id = ui.scope_id().with(case);
                        rect = match case {
                            "canvas" => canvas_value_field(ui, id, &mut value, false).rect,
                            _ => placement_field(ui, id, &mut value, "").rect,
                        };
                    });
                });
            });
            output.textures_delta.clear();
            rect.width()
        };
        let number = width("12.5 mm");
        assert!((width("4") - number).abs() < 0.5, "{case}: a number keeps the field's width");
        let formula = width("CellWidth * X + 2 * Wall");
        assert!(formula > number * 1.5, "{case}: {formula} for a formula, {number} for a number");
        let long = width("CellWidth * X + 2 * WallThickness - Clearance / 2 + LipHeight * Y");
        assert!(long > formula && long <= number * 3.0 + 1.0, "{case}: {long} capped at three times {number}");
        assert!((width("4") - number).abs() < 0.5, "{case}: and back to a number's width");
    }
}

#[test]
fn placement_field_keeps_focus_for_owner_validation_on_enter() {
    let ctx = crate::ui_kit::test_support::context();
    let id = egui::Id::unique("placement_enter");
    let mut value = String::from("invalid expression");
    let mut draw = |ui: &mut Ui| {
        row(ui, "Distance", |ui| {
            placement_field(ui, id, &mut value, "Distance");
        });
    };
    frame(&ctx, vec![], &mut draw);
    let at = registry::handle_at("dialog:Distance").unwrap();
    frame(&ctx, pointer(at, true), &mut draw);
    frame(&ctx, pointer(at, false), &mut draw);
    frame(&ctx, key(egui::Key::Enter), &mut draw);
    assert!(ctx.memory(|m| m.has_focus(id)), "owner can reject Enter without losing the editor");
    frame(&ctx, vec![egui::Event::Text("x".into())], &mut draw);
    assert!(value.contains('x'));
}

/// #10582: an angle typed a key at a time keeps its decimal point when its owner keeps it in
/// radians, as Edit Canvas does, and so gives back a hair off the value the field set.
#[test]
fn an_angle_kept_in_radians_takes_a_typed_decimal_point() {
    let ctx = crate::ui_kit::test_support::context();
    let mut radians = 0.0_f64;
    let mut draw = |ui: &mut Ui| {
        row(ui, "Angle", |ui| {
            let mut degrees = radians.to_degrees();
            if angle_field(ui, &mut degrees).changed() {
                radians = degrees.to_radians();
            }
        });
    };
    frame(&ctx, vec![], &mut draw);
    let at = registry::rect_of("dialog:angle:Angle").expect("the angle field").center();
    frame(&ctx, pointer(at, true), &mut draw);
    frame(&ctx, pointer(at, false), &mut draw);
    for typed in ["-", "3", ".", "7"] {
        frame(&ctx, vec![egui::Event::Text(typed.into())], &mut draw);
    }
    assert!((radians.to_degrees() + 3.7).abs() < 1e-9, "typed -3.7, the angle is {}", radians.to_degrees());
}

#[test]
fn bounded_name_editor_keeps_unicode_character_cap() {
    let ctx = crate::ui_kit::test_support::context();
    let id = egui::Id::unique("bounded_name");
    let mut value = String::new();
    let mut draw = |ui: &mut Ui| {
        row(ui, "Name", |ui| {
            bounded_cell_editor(ui, id, &mut value, "", false, 3);
        });
    };
    frame(&ctx, vec![], &mut draw);
    let at = registry::handle_at("dialog:Name").unwrap();
    frame(&ctx, pointer(at, true), &mut draw);
    frame(&ctx, pointer(at, false), &mut draw);
    frame(&ctx, vec![egui::Event::Text("åβ界d".into())], &mut draw);
    assert_eq!(value, "åβ界");
}

#[test]
fn count_steps_stay_within_limits_and_keep_expressions() {
    let eval = |text: &str| -> Option<f64> {
        let text = text.trim();
        match text {
            "n" => Some(4.0),
            "n + 2" => Some(6.0),
            "n + 3" => Some(7.0),
            "n + 1" => Some(5.0),
            "n - 1" => Some(3.0),
            "half" => Some(2.5),
            _ => text.parse().ok(),
        }
    };
    let limits = Count { min: 2, max: 10, eval: &eval };
    let step = |text: &str, delta| super::step_count(text, delta, &limits);
    assert_eq!(step("6", 1).as_deref(), Some("7"));
    assert_eq!(step(" 6 ", -1).as_deref(), Some("5"));
    assert_eq!(step("2", -1), None, "the minimum holds");
    assert_eq!(step("10", 1), None, "the maximum holds");
    assert_eq!(step("40", -1).as_deref(), Some("10"), "a typed count past the limit steps back inside it");
    assert_eq!(step("0", 1).as_deref(), Some("2"));
    assert_eq!(step("3.5", 1).as_deref(), Some("4"), "between whole numbers: the next one up");
    assert_eq!(step("3.5", -1).as_deref(), Some("3"));
    assert_eq!(step("n", 1).as_deref(), Some("n + 1"));
    assert_eq!(step("n", -1).as_deref(), Some("n - 1"));
    assert_eq!(step("n + 2", 1).as_deref(), Some("n + 3"));
    assert_eq!(step("n + 1", -1).as_deref(), Some("n"));
    assert_eq!(step("half", 1), None, "an expression between whole numbers is left alone");
    for junk in ["", "abc", "1e400", "-1e300", "NaN"] {
        assert_eq!(step(junk, 1), None, "{junk}");
    }
    let wide = Count { min: i64::MIN, max: i64::MAX, eval: &eval };
    assert_eq!(super::step_count("1e16", 1, &wide), None, "values past what counts can be are refused");
    assert_eq!(super::step_count("5", i64::MAX, &wide).as_deref(), Some("9223372036854775807"), "steps saturate rather than overflow");
    assert_eq!(super::step_count("5", i64::MIN, &wide).as_deref(), Some("-9223372036854775803"));
}

/// A count's step arrows stand inside the editor, which asks for the text cursor across its
/// whole rect, so the arrows showed an I-beam and read as part of the text. They are not text:
/// the pointer over one is the ordinary cursor, whether it can step or not, and the number
/// beside them is still typed.
#[test]
fn count_step_arrows_do_not_take_the_text_cursor() {
    for value in ["5", "1"] {
        let ctx = crate::ui_kit::test_support::context();
        let eval = |text: &str| text.trim().parse::<f64>().ok();
        let mut count = value.to_string();
        let mut draw = |ui: &mut Ui| {
            count_row(ui, "Instances", &mut count, &Count { min: 1, max: 10, eval: &eval });
        };
        frame(&ctx, vec![], &mut draw);
        let at = |name: &str| registry::handle_at(&format!("dialog:Instances {name}")).unwrap_or_else(|| panic!("{name} has a handle"));
        let (above, below) = (at("up"), at("down"));
        let field = registry::rect_of("dialog:widget:Instances").expect("the field has a rect");
        // The first frame registers the rects; the second reads the cursor they ask for. At 1 the
        // down arrow is dimmed and the up arrow can still step: neither is text either way.
        for (pos, which) in [(above, "up"), (below, "down")] {
            frame_cursor(&ctx, pos, &mut draw);
            assert_eq!(frame_cursor(&ctx, pos, &mut draw), egui::CursorIcon::Default, "{value}: the {which} arrow");
        }
        let text = egui::pos2(field.left() + 2.0, field.center().y);
        frame_cursor(&ctx, text, &mut draw);
        assert_eq!(frame_cursor(&ctx, text, &mut draw), egui::CursorIcon::Text, "{value}: the number is still typed");
        // The enabled arrow still steps when it is clicked.
        frame(&ctx, pointer(above, true), &mut draw);
        frame(&ctx, pointer(above, false), &mut draw);
        assert_eq!(count, if value == "1" { "2" } else { "6" }, "the up arrow steps");
    }
}

#[test]
fn selection_field_has_three_distinct_looks_in_every_theme() {
    type Look = (Vec<(egui::Color32, egui::Color32)>, Option<egui::Color32>);
    let look = |active: bool, filled: bool| -> Look {
        let ctx = crate::ui_kit::test_support::context();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.set_width(280.0);
            scope(ui, |ui| {
                row(ui, "Profiles", |ui| {
                    selection_field(ui, "1 selected", Selection { active, filled });
                });
            });
        });
        output.textures_delta.clear();
        let field = registry::rect_of("dialog:field:Profiles").unwrap();
        let rects = output
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Rect(r) if field.contains_rect(r.rect) => Some((r.fill, r.stroke.color)),
                _ => None,
            })
            .collect();
        let ink = output.shapes.iter().find_map(|s| match &s.shape {
            egui::Shape::Text(text) if text.galley.job.text == "1 selected" => Some(text.fallback_color),
            _ => None,
        });
        (rects, ink)
    };
    let mut palettes: Vec<(String, Option<crate::theme::FilePalette>, bool)> = vec![("dark".into(), None, true), ("light".into(), None, false)];
    for theme in crate::theme_files::builtin() {
        let dark = theme.base.is_dark();
        palettes.push((
            theme.id.clone(),
            Some(crate::theme::FilePalette {
                dark,
                tokens: crate::theme_files::resolve(theme),
                icons: crate::theme_files::resolve_icon_palette(theme, &Default::default()),
            }),
            dark,
        ));
    }
    let previous = solidcraft_theme::is_dark();
    for (name, custom, dark) in palettes {
        solidcraft_theme::set_dark(dark);
        crate::theme::set_file_palette(custom);
        let t = solidcraft_theme::Tokens::get();
        let [active_filled, active_empty, filled, empty] = [(true, true), (true, false), (false, true), (false, false)].map(|(a, f)| look(a, f));
        let accented = |l: &Look| l.0.iter().any(|&(fill, stroke)| stroke == t.form_primary || fill == t.selected_bg);
        assert!(accented(&active_filled) && accented(&active_empty), "{name}: the active input is accented");
        assert!(!accented(&filled), "{name}: a filled inactive input is not accented: {filled:?}");
        assert!(!accented(&empty), "{name}: an empty inactive input is not accented: {empty:?}");
        let outline = |l: &Look| l.0.last().copied().unwrap();
        // Two colours can't give filled and empty their own fill and border on top of the accent;
        // in 1-bit and 1-bit Dark the text itself tells them apart (a count against the prompt).
        if crate::theme_files::is_two_colour(&name) {
            // Inactive inputs are the page with an ink border: white on black in 1-bit, the
            // reverse in 1-bit Dark.
            let (paper, ink) = if dark { (egui::Color32::BLACK, egui::Color32::WHITE) } else { (egui::Color32::WHITE, egui::Color32::BLACK) };
            for look in [&active_filled, &active_empty, &filled, &empty] {
                for &(fill, border) in &look.0 {
                    for color in [fill, border] {
                        assert!(matches!(color, egui::Color32::BLACK | egui::Color32::WHITE), "{name}: {look:?}");
                    }
                }
            }
            for look in [&active_filled, &active_empty] {
                assert_eq!(outline(look).0, t.selected_bg, "{name}");
                assert_eq!(outline(look).0, ink, "{name}: the active input is inverted");
                assert_eq!(look.1, Some(t.selected_ink), "{name}");
            }
            for look in [&filled, &empty] {
                assert_eq!(outline(look), (paper, ink), "{name}");
            }
        } else {
            assert_ne!(outline(&filled).0, outline(&empty).0, "{name}: filled and empty fills differ");
            assert_ne!(outline(&filled).1, outline(&empty).1, "{name}: filled and empty borders differ");
        }
        assert_eq!(filled.1, Some(t.form_ink), "{name}: a filled input's count is in text colour");
        assert_eq!(empty.1, Some(t.form_dim), "{name}: an empty input's prompt is dim");
    }
    crate::theme::set_file_palette(None);
    solidcraft_theme::set_dark(previous);
}

#[test]
fn navigation_dropdown_popups_own_escape_and_return_focus_without_canceling_the_window() {
    use std::cell::Cell;
    let previous = solidcraft_theme::is_dark();
    for dark in [false, true] {
        solidcraft_theme::set_dark(dark);
        for kind in ["plain", "compact", "bounded", "editable"] {
            let ctx = crate::ui_kit::test_support::context();
            let action = Cell::new(Action::None);
            let trigger = Cell::new(None);
            let selected = Cell::new(0);
            let mut draw = |ui: &mut Ui| {
                let ctx = ui.ctx().clone();
                action.set(
                    Window::new(egui::Id::unique(("dropdown_escape", kind)), "settings", "Preferences")
                        .cards(1.0)
                        .fields()
                        .actions("OK", "Cancel", None)
                        .show(&ctx, true, |ui| {
                            // An earlier, closed dropdown must not steal this popup's Escape or focus.
                            row(ui, "Units", |ui| {
                                dropdown(ui, "units", &mut 0, &["mm", "in"]);
                            });
                            row(ui, "Visual style", |ui| {
                                let options = crate::navigation_slots::VISUAL_STYLES.map(|(_, label)| label);
                                let mut choice = selected.get();
                                let choices = |ui: &mut Ui| {
                                    for (index, option) in options.iter().enumerate() {
                                        let response = ui.selectable_label(selected.get() == index, *option);
                                        registry::publish_target(&format!("dialog:Visual style:{option}"), response.rect);
                                        if response.clicked() {
                                            selected.set(index);
                                        }
                                    }
                                };
                                let response = match kind {
                                    "plain" => dropdown(ui, "styles", &mut choice, &options),
                                    "compact" => compact_dropdown_menu(ui, "styles", options[selected.get()], choices),
                                    "bounded" => bounded_dropdown_menu(ui, "styles", options[selected.get()], 120.0, choices),
                                    _ => dropdown_menu_with_close_behavior(
                                        ui,
                                        "styles",
                                        options[selected.get()],
                                        egui::PopupCloseBehavior::CloseOnClickOutside,
                                        choices,
                                    ),
                                };
                                trigger.set(Some(response.id));
                                if kind == "plain" {
                                    selected.set(choice);
                                }
                            });
                        })
                        .1,
                );
            };
            for _ in 0..3 {
                frame(&ctx, vec![], &mut draw);
            }
            let at = registry::handle_at("dialog:Visual style").unwrap();
            frame(&ctx, pointer(at, true), &mut draw);
            frame(&ctx, pointer(at, false), &mut draw);
            frame(&ctx, vec![], &mut draw);
            assert!(registry::handle_at("dialog:Visual style:Wireframe").is_some(), "{kind}: popup opens");
            frame(&ctx, key(egui::Key::Escape), &mut draw);
            assert_eq!(action.get(), Action::None, "{kind}: first Escape belongs to the popup");
            assert!(ctx.memory(|m| m.has_focus(trigger.get().unwrap())), "{kind}: Escape refocuses its own trigger");
            frame(&ctx, vec![], &mut draw);
            assert!(registry::handle_at("dialog:Visual style:Wireframe").is_none());
            frame(&ctx, key(egui::Key::Enter), &mut draw);
            assert_eq!(action.get(), Action::None, "{kind}: Enter reopens rather than submitting");
            frame(&ctx, vec![], &mut draw);
            assert!(registry::handle_at("dialog:Visual style:Wireframe").is_some(), "{kind}: focused trigger reopens on Enter");
            frame(&ctx, key(egui::Key::Escape), &mut draw);
            assert_eq!(action.get(), Action::None);
            frame(&ctx, vec![], &mut draw);
            assert!(registry::handle_at("dialog:Visual style:Wireframe").is_none());
            frame(&ctx, key(egui::Key::Escape), &mut draw);
            assert_eq!(action.get(), Action::Cancel, "{kind}: Escape without a popup cancels normally");
            assert_eq!(selected.get(), 0, "dismissal does not select an option");
        }
    }
    solidcraft_theme::set_dark(previous);
}

// The text fields' right-click menu (#8875).

use super::super::text_menu::{self, Edit, Field, State, TEST_CLIPBOARD};
use std::cell::{Cell, RefCell};

/// A frame of the kit harness that hands back its output (copied text, the menu's handles).
fn menu_frame(ctx: &egui::Context, events: Vec<egui::Event>, mut add: impl FnMut(&mut Ui)) -> egui::FullOutput {
    registry::clear_handles();
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(320.0, 400.0))),
        events,
        focused: true,
        ..Default::default()
    };
    crate::ui_kit::test_support::raw_frame(ctx, input, |ui| {
        ui.set_width(280.0);
        scope(ui, &mut add);
    })
}

fn right(pos: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(pos),
        egui::Event::PointerButton { pos, button: egui::PointerButton::Secondary, pressed, modifiers: Default::default() },
    ]
}

/// Right-click at `at`: the menu is open after it, laid out (a popup's first frame only sizes it).
fn right_click(ctx: &egui::Context, at: egui::Pos2, add: &mut impl FnMut(&mut Ui)) {
    menu_frame(ctx, right(at, true), &mut *add);
    menu_frame(ctx, right(at, false), &mut *add);
    menu_frame(ctx, vec![], &mut *add);
}

/// Click the menu row `row` (`text_paste`, ...); returns what the click put on the clipboard.
fn pick(ctx: &egui::Context, row: &str, add: &mut impl FnMut(&mut Ui)) -> Vec<String> {
    let at = registry::handle_at(&format!("menu:{row}")).unwrap_or_else(|| panic!("the menu shows {row}"));
    menu_frame(ctx, pointer(at, true), &mut *add);
    let output = menu_frame(ctx, pointer(at, false), &mut *add);
    let copied = output
        .platform_output
        .commands
        .iter()
        .filter_map(|c| if let egui::OutputCommand::CopyText(t) = c { Some(t.clone()) } else { None })
        .collect();
    // One more frame: the field shows the focus the menu gave back.
    menu_frame(ctx, vec![], &mut *add);
    copied
}

fn select(ctx: &egui::Context, id: egui::Id, from: usize, to: usize) {
    let mut state = egui::text_edit::TextEditState::load(ctx, id).unwrap_or_default();
    state.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(from), egui::text::CCursor::new(to))));
    state.store(ctx, id);
}

fn selection(ctx: &egui::Context, id: egui::Id) -> Option<(usize, usize)> {
    let range = egui::text_edit::TextEditState::load(ctx, id)?.cursor.char_range()?;
    let [a, b] = range.sorted_cursors();
    Some((a.index.0, b.index.0))
}

fn set_clipboard(text: Option<&str>) {
    TEST_CLIPBOARD.with(|c| *c.borrow_mut() = text.map(str::to_owned));
}

#[test]
fn a_right_click_on_every_kind_of_text_field_opens_its_menu() {
    set_clipboard(None);
    for case in ["expression", "plain", "cell", "placement", "canvas", "password", "multiline", "area", "message", "read-only", "inline"] {
        let ctx = crate::ui_kit::test_support::context();
        let value = RefCell::new("12 mm".to_owned());
        let rect = Cell::new(egui::Rect::NOTHING);
        let mut draw = |ui: &mut Ui| {
            let id = ui.scope_id().with(case);
            let value = &mut *value.borrow_mut();
            rect.set(match case {
                "expression" | "plain" => super::editor(ui, value, "", case == "expression").2.rect,
                "cell" => cell_editor(ui, id, value, "", false).rect,
                "placement" => placement_field(ui, id, value, "").rect,
                "canvas" => canvas_value_field(ui, id, value, false).rect,
                "password" => password_field(ui, id, value, "").rect,
                "multiline" => multiline_text_field(ui, "Text", id, value).rect,
                "area" => text_area(ui, "Text", id, value, 3, 100).rect,
                "message" => message_field(ui, id, value, "").rect,
                "read-only" => read_only_text(ui, "12 mm").rect,
                _ => inline_name(ui, egui::Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(200.0, 22.0)), id, value).rect,
            });
        };
        menu_frame(&ctx, vec![], &mut draw);
        assert!(registry::handle_at("menu:text_paste").is_none(), "{case}: no menu before the click");
        right_click(&ctx, rect.get().center(), &mut draw);
        for row in ["text_cut", "text_copy", "text_paste", "text_select_all"] {
            assert!(registry::handle_at(&format!("menu:{row}")).is_some(), "{case}: the menu shows {row}");
        }
        assert_eq!(*value.borrow(), "12 mm", "{case}: opening the menu changes nothing");
    }
}

#[test]
fn paste_inserts_at_the_cursor_and_replaces_the_selection() {
    let ctx = crate::ui_kit::test_support::context();
    let id = egui::Id::unique("paste_dimension");
    let value = RefCell::new("12 mm".to_owned());
    let rect = Cell::new(egui::Rect::NOTHING);
    let changed = Cell::new(false);
    let mut draw = |ui: &mut Ui| {
        let r = cell_editor(ui, id, &mut value.borrow_mut(), "", false);
        rect.set(r.rect);
        changed.set(changed.get() | r.changed());
    };
    menu_frame(&ctx, vec![], &mut draw);
    menu_frame(&ctx, pointer(rect.get().center(), true), &mut draw);
    menu_frame(&ctx, pointer(rect.get().center(), false), &mut draw);
    assert!(ctx.memory(|m| m.has_focus(id)));
    select(&ctx, id, 2, 2);
    menu_frame(&ctx, vec![], &mut draw);
    set_clipboard(Some("3"));
    let end = egui::pos2(rect.get().right() - 2.0, rect.get().center().y);
    right_click(&ctx, end, &mut draw);
    // A right press moves the caret, as a left press does: to the end of the text here.
    assert_eq!(selection(&ctx, id), Some((5, 5)));
    select(&ctx, id, 2, 2);
    assert!(pick(&ctx, "text_paste", &mut draw).is_empty(), "Paste copies nothing");
    assert_eq!(*value.borrow(), "123 mm", "Paste inserts the clipboard's text at the caret");
    assert!(changed.get(), "the caller sees the paste as a change");
    assert_eq!(selection(&ctx, id), Some((3, 3)), "the caret follows the pasted text");
    assert!(ctx.memory(|m| m.has_focus(id)), "the field keeps the focus, so it neither commits nor closes");

    // Select all, then right-click inside the selection: it stays, and Paste replaces it.
    right_click(&ctx, rect.get().center(), &mut draw);
    pick(&ctx, "text_select_all", &mut draw);
    assert_eq!(selection(&ctx, id), Some((0, 6)));
    set_clipboard(Some("4 mm\r\n"));
    right_click(&ctx, rect.get().center(), &mut draw);
    assert_eq!(selection(&ctx, id), Some((0, 6)), "a right press inside the selection keeps it");
    pick(&ctx, "text_paste", &mut draw);
    assert_eq!(*value.borrow(), "4 mm  ", "Paste replaces the selection; a single line takes line breaks as spaces");
    set_clipboard(None);
}

#[test]
fn copy_and_cut_put_the_selection_on_the_clipboard() {
    set_clipboard(None);
    let ctx = crate::ui_kit::test_support::context();
    let id = egui::Id::unique("copy_cut");
    let value = RefCell::new("width + 2 mm".to_owned());
    let rect = Cell::new(egui::Rect::NOTHING);
    let mut draw = |ui: &mut Ui| rect.set(cell_editor(ui, id, &mut value.borrow_mut(), "", false).rect);
    menu_frame(&ctx, vec![], &mut draw);
    menu_frame(&ctx, pointer(rect.get().center(), true), &mut draw);
    menu_frame(&ctx, pointer(rect.get().center(), false), &mut draw);
    select(&ctx, id, 0, 12);
    menu_frame(&ctx, vec![], &mut draw);
    right_click(&ctx, rect.get().center(), &mut draw);
    assert_eq!(pick(&ctx, "text_copy", &mut draw), ["width + 2 mm"]);
    assert_eq!(*value.borrow(), "width + 2 mm", "Copy leaves the text");
    select(&ctx, id, 0, 5);
    menu_frame(&ctx, vec![], &mut draw);
    // The press lands inside "width", the first characters at the field's left.
    right_click(&ctx, egui::pos2(rect.get().left() + 12.0, rect.get().center().y), &mut draw);
    assert_eq!(pick(&ctx, "text_cut", &mut draw), ["width"]);
    assert_eq!(*value.borrow(), " + 2 mm", "Cut removes the selection");
    assert_eq!(selection(&ctx, id), Some((0, 0)));
}

#[test]
fn the_menu_offers_only_what_the_field_and_its_selection_allow() {
    set_clipboard(Some("5"));
    let ctx = crate::ui_kit::test_support::context();
    let id = egui::Id::unique("nothing_selected");
    let value = RefCell::new("12".to_owned());
    let rect = Cell::new(egui::Rect::NOTHING);
    let mut draw = |ui: &mut Ui| rect.set(cell_editor(ui, id, &mut value.borrow_mut(), "", false).rect);
    menu_frame(&ctx, vec![], &mut draw);
    // No selection: Cut and Copy do nothing.
    right_click(&ctx, rect.get().center(), &mut draw);
    assert!(pick(&ctx, "text_copy", &mut draw).is_empty());
    right_click(&ctx, rect.get().center(), &mut draw);
    assert!(pick(&ctx, "text_cut", &mut draw).is_empty());
    assert_eq!(*value.borrow(), "12");

    // Read-only text: Paste leaves it as it is; Select All and Copy still work.
    let ctx = crate::ui_kit::test_support::context();
    let rect = Cell::new(egui::Rect::NOTHING);
    let mut draw = |ui: &mut Ui| rect.set(read_only_text(ui, "report line").rect);
    menu_frame(&ctx, vec![], &mut draw);
    right_click(&ctx, rect.get().center(), &mut draw);
    pick(&ctx, "text_paste", &mut draw);
    right_click(&ctx, rect.get().center(), &mut draw);
    pick(&ctx, "text_select_all", &mut draw);
    right_click(&ctx, rect.get().center(), &mut draw);
    assert_eq!(pick(&ctx, "text_copy", &mut draw), ["report line"]);
    set_clipboard(None);

    let on = |state: State| Edit::ALL.map(|e| text_menu::enabled(e, state));
    let all = State { editable: true, selected: true, secret: false, pastable: true, text: true };
    assert_eq!(on(all), [true; 4]);
    assert_eq!(on(State { selected: false, ..all }), [false, false, true, true], "nothing selected");
    assert_eq!(on(State { editable: false, ..all }), [false, true, false, true], "read-only or disabled");
    assert_eq!(on(State { secret: true, ..all }), [false, false, true, true], "a password is never copied");
    assert_eq!(on(State { pastable: false, ..all }), [true, true, false, true], "an empty clipboard");
    assert_eq!(on(State::default()), [false; 4], "an empty read-only field");

    // The edits themselves, on the buffer.
    let mut text = "abcdef".to_owned();
    let range = |a: usize, b: usize| Some(egui::text::CCursorRange::two(egui::text::CCursor::new(a), egui::text::CCursor::new(b)));
    let done = text_menu::apply(&mut text, range(1, 3), Edit::Paste, Some("XYZ"), Field::LINE.limited(6));
    assert_eq!((text.as_str(), done.changed), ("aXYdef", true), "Paste replaces the selection within the character limit");
    let done = text_menu::apply(&mut text, range(40, 90), Edit::Cut, None, Field::LINE);
    assert_eq!((text.as_str(), done.copied), ("aXYdef", None), "a stale selection past the end selects nothing");
    let done = text_menu::apply(&mut text, range(0, 2), Edit::Copy, None, Field { secret: true, ..Field::LINE });
    assert_eq!(done.copied, None, "a password is never copied");
    let done = text_menu::apply(&mut text, None, Edit::Paste, Some("a\nb"), Field::LINES);
    assert_eq!(text, "aXYdefa\nb", "without a caret Paste appends; a multiline field keeps line breaks");
    assert!(done.changed);
}

#[test]
fn a_right_click_on_a_value_box_over_the_canvas_does_not_reach_the_canvas() {
    set_clipboard(None);
    let ctx = crate::ui_kit::test_support::context();
    let id = egui::Id::unique("canvas_box");
    let value = RefCell::new("20".to_owned());
    let (field, canvas_menu) = (Cell::new(egui::Rect::NOTHING), Cell::new(false));
    let mut draw = |ui: &mut Ui| {
        // The viewport: a right-click on it opens its own menu.
        let canvas = ui.allocate_rect(ui.max_rect(), egui::Sense::click_and_drag());
        canvas_menu.set(canvas_menu.get() | canvas.secondary_clicked());
        let ctx = ui.ctx().clone();
        let area = Container::new(&ctx, Surface::Canvas).area(egui::Id::unique("canvas_box_area"));
        area.fixed_pos(egui::pos2(60.0, 60.0)).order(egui::Order::Foreground).show(&ctx, |ui| {
            field.set(canvas_value_field(ui, id, &mut value.borrow_mut(), false).rect);
        });
    };
    menu_frame(&ctx, vec![], &mut draw);
    menu_frame(&ctx, vec![], &mut draw);
    right_click(&ctx, field.get().center(), &mut draw);
    assert!(registry::handle_at("menu:text_paste").is_some(), "the value box's menu opens");
    assert!(!canvas_menu.get(), "the canvas does not see the right-click");
    menu_frame(&ctx, key(egui::Key::Escape), &mut draw);
    menu_frame(&ctx, vec![], &mut draw);
    right_click(&ctx, egui::pos2(250.0, 300.0), &mut draw);
    assert!(canvas_menu.get(), "a right-click beside the box still reaches the canvas");
    assert!(registry::handle_at("menu:text_paste").is_none(), "and opens no text menu");
}

#[test]
fn esc_closes_the_text_menu_alone() {
    set_clipboard(None);
    let ctx = crate::ui_kit::test_support::context();
    let id = egui::Id::unique("esc_dimension");
    let value = RefCell::new("12".to_owned());
    let (rect, owner_saw_esc, open) = (Cell::new(egui::Rect::NOTHING), Cell::new(false), Cell::new(false));
    let mut draw = |ui: &mut Ui| {
        rect.set(cell_editor(ui, id, &mut value.borrow_mut(), "", false).rect);
        // What an expression field's completion asks, right after the field.
        open.set(text_menu::menu_open(ui.ctx(), id));
        // A sketch dimension's box cancels on Esc.
        owner_saw_esc.set(owner_saw_esc.get() | ui.input(|i| i.key_pressed(egui::Key::Escape)));
    };
    menu_frame(&ctx, vec![], &mut draw);
    menu_frame(&ctx, pointer(rect.get().center(), true), &mut draw);
    menu_frame(&ctx, pointer(rect.get().center(), false), &mut draw);
    right_click(&ctx, rect.get().center(), &mut draw);
    assert!(open.get(), "an expression field's suggestions wait while the menu is open");
    menu_frame(&ctx, key(egui::Key::Escape), &mut draw);
    menu_frame(&ctx, vec![], &mut draw);
    assert!(registry::handle_at("menu:text_paste").is_none(), "Esc closes the menu");
    assert!(!open.get(), "and the suggestions may show again");
    assert!(!owner_saw_esc.get(), "the field's owner doesn't see the Esc that closed the menu");
    assert!(ctx.memory(|m| m.has_focus(id)), "and the field keeps the focus");
    menu_frame(&ctx, key(egui::Key::Escape), &mut draw);
    assert!(owner_saw_esc.get(), "with the menu closed, Esc reaches the owner again");
}

#[test]
fn bounded_dropdown_focus_announces_full_selected_source_with_a_nonempty_accessible_label() {
    use std::cell::Cell;
    fn visible_text(shape: &egui::Shape, clip: egui::Rect, value: &str) -> bool {
        match shape {
            egui::Shape::Text(text) => text.galley.text() == value && clip.contains(text.pos),
            egui::Shape::Vec(shapes) => shapes.iter().any(|shape| visible_text(shape, clip, value)),
            _ => false,
        }
    }
    let long_source = "CPU retained image with a complete captured shot name that exceeds the bounded toolbar width";
    for variant in ["bounded", "regular", "compact", "outside"] {
        let current = if variant == "bounded" { long_source } else { "Add Light" };
        for labelled in [false, true] {
            let ctx = crate::ui_kit::test_support::context();
            ctx.enable_accesskit();
            let id = Cell::new(None);
            let focus = Cell::new(false);
            let rect = Cell::new(egui::Rect::NOTHING);
            let chosen = Cell::new(false);
            let mut draw = |ui: &mut Ui| {
                let picker = |ui: &mut Ui| {
                    if focus.get() {
                        // Give the real widget focus during this frame, before its
                        // native and kit metadata are emitted. No synthetic event.
                        ui.memory_mut(|memory| memory.request_focus(id.get().expect("the first frame captures the picker ID")));
                    }
                    let choices = |ui: &mut Ui| {
                        if list_picker(ui, "source", current, true).clicked() {
                            chosen.set(true);
                            ui.close();
                        }
                    };
                    let response = match variant {
                        "bounded" => bounded_dropdown_menu(ui, "accessible_bounded_source", current, 120.0, choices),
                        "regular" => dropdown_menu(ui, "accessible_regular_source", current, choices),
                        "compact" => compact_dropdown_menu(ui, "accessible_compact_source", current, choices),
                        _ => dropdown_menu_with_close_behavior(
                            ui,
                            "accessible_outside_source",
                            current,
                            egui::PopupCloseBehavior::CloseOnClickOutside,
                            choices,
                        ),
                    };
                    id.set(Some(response.id));
                    rect.set(response.rect);
                };
                if labelled {
                    row(ui, "Retained source", picker);
                } else {
                    picker(ui);
                }
            };
            menu_frame(&ctx, vec![], &mut draw);
            let initial_rect = rect.get();
            focus.set(true);
            let output = menu_frame(&ctx, vec![], &mut draw);
            assert!(ctx.memory(|memory| memory.has_focus(id.get().unwrap())));
            let event = output.platform_output.events.last().expect("focusing the actual picker must announce its value");
            assert!(matches!(event, egui::output::OutputEvent::FocusGained(_)), "the final announcement must describe actual focus: {event:?}");
            let info = event.widget_info();
            assert_eq!(info.role, egui::Role::ComboBox);
            assert!(info.enabled);
            let expected = if labelled { solidcraft_ui_text::tr_label("Retained source") } else { current };
            assert_eq!(info.label.as_deref(), Some(expected));
            assert!(!info.label.as_deref().unwrap().trim().is_empty());
            assert_eq!(
                info.current_text_value.as_deref(),
                Some(current),
                "the visual ellipsis cannot replace the full selected source in focus metadata"
            );
            let update = output.platform_output.accesskit_update.as_ref().expect("the native accessibility tree must describe the focused picker");
            let pickers: Vec<_> = update.nodes.iter().filter(|(_, node)| node.role() == egui::accesskit::Role::ComboBox).collect();
            assert_eq!(pickers.len(), 1, "the real picker has one native accessibility node");
            let node = &pickers[0].1;
            assert_eq!(node.label(), Some(expected));
            assert_eq!(node.value(), Some(current), "the native screen reader receives the complete selected source");
            assert!(!node.is_disabled());
            if variant != "bounded" {
                assert!(
                    output.shapes.iter().any(|shape| visible_text(&shape.shape, shape.clip_rect, current)),
                    "the accessible bare/row picker value is also the actual visible control text"
                );
            }
            assert_eq!(rect.get(), initial_rect, "accessibility focus leaves the actual dropdown layout unchanged");
            assert!(!chosen.get(), "focus cannot select an action");
            if !labelled {
                // egui's simple TTS description reads ComboBox labels; its native
                // AccessKit tree above also carries the separate selected value.
                assert!(output.platform_output.events_description().contains(current));
            }
            // The same real control remains clickable and its choice closes under
            // either dismissal policy. No test-synthesized accessibility event.
            focus.set(false);
            menu_frame(&ctx, pointer(rect.get().center(), true), &mut draw);
            menu_frame(&ctx, pointer(rect.get().center(), false), &mut draw);
            menu_frame(&ctx, vec![], &mut draw);
            assert!(egui::ComboBox::is_open(&ctx, id.get().unwrap()), "{variant}: actual dropdown opens");
            let choice = registry::handle_at("dialog:list:source").expect("the actual popup choice is visible");
            menu_frame(&ctx, pointer(choice, true), &mut draw);
            menu_frame(&ctx, pointer(choice, false), &mut draw);
            menu_frame(&ctx, vec![], &mut draw);
            assert!(chosen.get(), "{variant}: actual pointer activates the chosen action");
            assert!(!egui::ComboBox::is_open(&ctx, id.get().unwrap()), "{variant}: accepted action closes its popup");
            assert_eq!(rect.get(), initial_rect, "choice activation keeps the actual control geometry");
        }
    }
}

#[test]
fn a_track_number_adds_up_and_a_sign_adds_to_the_value_now() {
    let typed = super::relative_number;
    assert_eq!(typed("1200", 5.0), Some(1200.0));
    assert_eq!(typed("+30", 100.0), Some(130.0));
    assert_eq!(typed(" -30 ", 100.0), Some(70.0));
    assert_eq!(typed("\u{2212}2,5", 10.0), Some(7.5));
    assert_eq!(typed("0-5", 100.0), Some(-5.0));
    assert_eq!(typed("1e-3+1", 0.0), Some(1.001));
    assert_eq!(typed("abc", 1.0), None);
}

#[test]
fn a_label_drag_reads_only_literal_values_and_moves_them_on_the_step_grid() {
    let literal = |text: &str| super::literal(&crate::params_dialog::DocExpressions, text).map(|l| (l.number, l.unit, l.comma));
    assert_eq!(literal("20 mm"), Some((20.0, " mm".into(), false)));
    assert_eq!(literal(" 1,5in "), Some((1.5, "in".into(), true)));
    assert_eq!(literal("-3"), Some((-3.0, String::new(), false)));
    for not_a_value in ["width", "width + 2 mm", "2 * 3 mm", "", "1e3", "1e400 mm", "20 bananas"] {
        assert_eq!(literal(not_a_value), None, "{not_a_value:?} is left alone");
    }
    assert_eq!(super::scrub_step(20.0), 0.1);
    assert_eq!(super::scrub_step(250.0), 1.0);
    assert_eq!(super::scrub_step(0.0), 0.01);
    assert_eq!(super::scrub_step(-45.0), 0.1);
    assert!((super::scrubbed(20.0, 30.0, 0.1) - 23.0).abs() < 1e-9);
    assert!((super::scrubbed(20.37, 1.0, 0.1) - 20.5).abs() < 1e-9, "the value lands on the step's grid");
    assert_eq!(super::scrubbed(2.0, -500.0, 0.1), 0.0, "a size stops at zero");
    assert!((super::scrubbed(-2.0, -10.0, 0.1) + 3.0).abs() < 1e-9, "a negative offset keeps moving");
    assert_eq!(super::scrubbed(1.0, f32::MAX, 1e8), 1.0, "an overflowing drag keeps the value");
    let text = |n, step, typed: &str| super::scrub_text(n, step, &super::literal(&crate::params_dialog::DocExpressions, typed).unwrap());
    assert_eq!(text(23.0, 0.1, "20 mm"), "23.0 mm");
    assert_eq!(text(1.75, 0.01, "1,5 in"), "1,75 in");
    assert_eq!(text(-0.0001, 0.01, "-1"), "0.00");
    assert_eq!(text(300.0, 1.0, "250 mm"), "300 mm");
}

#[test]
fn dragging_a_value_rows_label_changes_its_literal_and_leaves_expressions_alone() {
    fn frame(ctx: &egui::Context, events: Vec<egui::Event>, modifiers: egui::Modifiers, values: &mut [String; 2]) {
        registry::clear_handles();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(320.0, 400.0))),
            events: std::iter::once(egui::Event::ModifiersChanged(modifiers)).chain(events).collect(),
            focused: true,
            ..Default::default()
        };
        crate::ui_kit::test_support::raw_frame(ctx, input, |ui| {
            ui.set_width(280.0);
            scope(ui, |ui| {
                let [distance, offset] = &mut *values;
                expr_row(ui, "Distance", distance);
                expr_row(ui, "Offset", offset);
            });
        });
    }
    fn drag(ctx: &egui::Context, row: &str, dx: f32, modifiers: egui::Modifiers, values: &mut [String; 2]) {
        let start = registry::rect_of(&format!("dialog:label:{row}")).unwrap().center();
        let end = start + egui::vec2(dx, 0.0);
        let button = |pos, pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers };
        frame(ctx, vec![egui::Event::PointerMoved(start), button(start, true)], modifiers, values);
        for step in 1..=4 {
            frame(ctx, vec![egui::Event::PointerMoved(start + egui::vec2(dx * step as f32 / 4.0, 0.0))], modifiers, values);
        }
        frame(ctx, vec![button(end, false)], modifiers, values);
        frame(ctx, vec![], modifiers, values);
    }
    let ctx = crate::ui_kit::test_support::context();
    crate::params_dialog::install_expressions(&ctx);
    let mut values = ["20 mm".to_owned(), "depth + 2 mm".to_owned()];
    frame(&ctx, vec![], Default::default(), &mut values);
    drag(&ctx, "Distance", 40.0, Default::default(), &mut values);
    assert_eq!(values[0], "24.0 mm", "40 points at 0.1 mm a point");
    drag(&ctx, "Distance", 40.0, egui::Modifiers::SHIFT, &mut values);
    assert_eq!(values[0], "24.40 mm", "Shift is ten times finer");
    drag(&ctx, "Distance", -20.0, egui::Modifiers::COMMAND, &mut values);
    assert_eq!(values[0], "4 mm", "Ctrl is ten times coarser");
    drag(&ctx, "Offset", 60.0, Default::default(), &mut values);
    assert_eq!(values[1], "depth + 2 mm", "an expression keeps its parameter");
}
