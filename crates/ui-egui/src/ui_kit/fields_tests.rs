use crate::ui_kit::test_support::*;
use crate::ui_kit::*;

use crate::scenario;
use egui::Ui;

#[test]
fn every_text_input_centres_its_galley_for_mono_and_proportional_fonts() {
    for case in ["expression", "plain", "cell", "invalid", "library", "placement", "password", "multiline", "area", "message"] {
        let ctx = crate::fonts::test_context();
        let mut value = "Centred".to_owned();
        let mut rect = egui::Rect::NOTHING;
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.set_width(280.0);
            scope(ui, |ui| {
                let id = ui.id().with(case);
                rect = match case {
                    "expression" | "plain" => super::editor(ui, &mut value, "", case == "expression").2.rect,
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
        if case == "placement" {
            let font = egui::FontId::proportional(crate::theme::Tokens::get().font_body);
            let native = ctx.fonts_mut(|fonts| fonts.row_height(&font))
                + ctx.style_of(egui::Theme::Light).spacing.extra_text_line_spacing
                + crate::theme::Tokens::get().space_4;
            assert_eq!(rect.height(), native, "canvas editor retains its native hit target");
        } else if !matches!(case, "multiline" | "area" | "message") {
            assert_eq!(rect.height(), crate::theme::Tokens::get().control_height, "{case}");
        }
    }
}

#[test]
fn tab_visits_expression_fields_in_row_order() {
    let ctx = crate::fonts::test_context();
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
    let first_pos = scenario::handle_at("dialog:First value").unwrap();
    frame(&ctx, pointer(first_pos, true), &mut draw);
    frame(&ctx, pointer(first_pos, false), &mut draw);
    frame(&ctx, key(egui::Key::Tab), &mut draw);
    frame(&ctx, vec![egui::Event::Text("2".to_string())], &mut draw);
    assert_eq!(first, "1");
    assert_eq!(second, "2");
}

#[test]
fn multiline_text_retains_focus_id_and_three_line_geometry() {
    let ctx = crate::fonts::test_context();
    let id = egui::Id::new("multiline_test");
    let mut value = "First\nSecond".to_string();
    let mut draw = |ui: &mut Ui| {
        multiline_text_field(ui, "Text", id, &mut value);
    };
    frame(&ctx, vec![], &mut draw);
    let row = scenario::rect_of("dialog:row:Text").unwrap();
    assert_eq!(row.height(), 3.0 * crate::theme::Tokens::get().control_height);
    let at = scenario::handle_at("dialog:Text").unwrap();
    frame(&ctx, pointer(at, true), &mut draw);
    frame(&ctx, pointer(at, false), &mut draw);
    assert!(ctx.memory(|m| m.has_focus(id)));
}

#[test]
fn searchable_dropdown_filters_selects_and_accepts_custom_values() {
    for custom in [false, true] {
        let ctx = crate::fonts::test_context();
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
        let at = scenario::handle_at("dialog:Font").unwrap();
        frame(&ctx, pointer(at, true), &mut draw);
        frame(&ctx, pointer(at, false), &mut draw);
        frame(&ctx, vec![], &mut draw);
        assert!(scenario::handle_at("dialog:Font search").is_some());
        assert!(scenario::handle_at("search:one").is_none(), "filter hides unmatched entries");
        let pick = scenario::handle_at(if custom { "dialog:Font file" } else { "search:two" }).unwrap();
        frame(&ctx, pointer(pick, true), &mut draw);
        frame(&ctx, pointer(pick, false), &mut draw);
        assert_eq!(chosen.as_deref(), Some(if custom { "/fonts/face.ttf" } else { "two" }));
    }
}

#[test]
fn preference_dropdown_keeps_disabled_choices_inert_and_closes_after_selection() {
    let ctx = crate::fonts::test_context();
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
    let at = scenario::handle_at("dialog:Samples").unwrap();
    frame(&ctx, pointer(at, true), &mut draw);
    frame(&ctx, pointer(at, false), &mut draw);
    let disabled = scenario::handle_at("dialog:list:4").unwrap();
    frame(&ctx, pointer(disabled, true), &mut draw);
    frame(&ctx, pointer(disabled, false), &mut draw);
    frame(&ctx, vec![], &mut draw);
    assert_eq!(picked.get(), 1, "a disabled choice must not change the value");
    // Reopen if the native popup closed on the inside click.
    if scenario::handle_at("dialog:list:8").is_none() {
        let at = scenario::handle_at("dialog:Samples").unwrap();
        frame(&ctx, pointer(at, true), &mut draw);
        frame(&ctx, pointer(at, false), &mut draw);
        frame(&ctx, vec![], &mut draw);
    }
    let enabled = scenario::handle_at("dialog:list:8").unwrap();
    frame(&ctx, pointer(enabled, true), &mut draw);
    frame(&ctx, pointer(enabled, false), &mut draw);
    frame(&ctx, vec![], &mut draw);
    assert!(scenario::handle_at("dialog:list:8").is_none());
    assert_eq!(picked.get(), 8);
}

#[test]
fn secret_field_retains_focus_and_masks_paint_and_accessibility_events() {
    let ctx = crate::fonts::test_context();
    let id = egui::Id::new("secret_test");
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
    let ctx = crate::fonts::test_context();
    let id = egui::Id::new("message_test");
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
        let ctx = crate::fonts::test_context();
        let width = |value: &str| {
            let mut value = value.to_owned();
            let mut rect = egui::Rect::NOTHING;
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                egui::Area::new(egui::Id::new("grow")).fixed_pos(egui::pos2(10.0, 10.0)).show(ui.ctx(), |ui| {
                    ui.horizontal(|ui| {
                        let id = ui.id().with(case);
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
    let ctx = crate::fonts::test_context();
    let id = egui::Id::new("placement_enter");
    let mut value = String::from("invalid expression");
    let mut draw = |ui: &mut Ui| {
        row(ui, "Distance", |ui| {
            placement_field(ui, id, &mut value, "Distance");
        });
    };
    frame(&ctx, vec![], &mut draw);
    let at = scenario::handle_at("dialog:Distance").unwrap();
    frame(&ctx, pointer(at, true), &mut draw);
    frame(&ctx, pointer(at, false), &mut draw);
    frame(&ctx, key(egui::Key::Enter), &mut draw);
    assert!(ctx.memory(|m| m.has_focus(id)), "owner can reject Enter without losing the editor");
    frame(&ctx, vec![egui::Event::Text("x".into())], &mut draw);
    assert!(value.contains('x'));
}

#[test]
fn bounded_name_editor_keeps_unicode_character_cap() {
    let ctx = crate::fonts::test_context();
    let id = egui::Id::new("bounded_name");
    let mut value = String::new();
    let mut draw = |ui: &mut Ui| {
        row(ui, "Name", |ui| {
            bounded_cell_editor(ui, id, &mut value, "", false, 3);
        });
    };
    frame(&ctx, vec![], &mut draw);
    let at = scenario::handle_at("dialog:Name").unwrap();
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

#[test]
fn selection_field_has_three_distinct_looks_in_every_theme() {
    type Look = (Vec<(egui::Color32, egui::Color32)>, Option<egui::Color32>);
    let look = |active: bool, filled: bool| -> Look {
        let ctx = crate::fonts::test_context();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.set_width(280.0);
            scope(ui, |ui| {
                row(ui, "Profiles", |ui| {
                    selection_field(ui, "1 selected", Selection { active, filled });
                });
            });
        });
        output.textures_delta.clear();
        let field = scenario::rect_of("dialog:field:Profiles").unwrap();
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
    let mut palettes: Vec<(String, Option<crate::theme::Custom>, bool)> = vec![("dark".into(), None, true), ("light".into(), None, false)];
    for theme in crate::theme_files::builtin() {
        let dark = theme.base.is_dark();
        palettes.push((theme.id.clone(), Some(crate::theme::Custom { dark, tokens: crate::theme_files::resolve(theme) }), dark));
    }
    let previous = crate::theme::is_dark();
    for (name, custom, dark) in palettes {
        crate::theme::set_dark(dark);
        crate::theme::set_custom(custom);
        let t = crate::theme::Tokens::get();
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
    crate::theme::set_custom(None);
    crate::theme::set_dark(previous);
}

// The text fields' right-click menu (#8875).

use super::super::text_menu::{self, Edit, Field, State, TEST_CLIPBOARD};
use std::cell::{Cell, RefCell};

/// A frame of the kit harness that hands back its output (copied text, the menu's handles).
fn menu_frame(ctx: &egui::Context, events: Vec<egui::Event>, mut add: impl FnMut(&mut Ui)) -> egui::FullOutput {
    scenario::clear_handles();
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(320.0, 400.0))),
        events,
        focused: true,
        ..Default::default()
    };
    crate::test_support::frame(ctx, input, |ui| {
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
    let at = scenario::handle_at(&format!("menu:{row}")).unwrap_or_else(|| panic!("the menu shows {row}"));
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
        let ctx = crate::fonts::test_context();
        let value = RefCell::new("12 mm".to_owned());
        let rect = Cell::new(egui::Rect::NOTHING);
        let mut draw = |ui: &mut Ui| {
            let id = ui.id().with(case);
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
        assert!(scenario::handle_at("menu:text_paste").is_none(), "{case}: no menu before the click");
        right_click(&ctx, rect.get().center(), &mut draw);
        for row in ["text_cut", "text_copy", "text_paste", "text_select_all"] {
            assert!(scenario::handle_at(&format!("menu:{row}")).is_some(), "{case}: the menu shows {row}");
        }
        assert_eq!(*value.borrow(), "12 mm", "{case}: opening the menu changes nothing");
    }
}

#[test]
fn paste_inserts_at_the_cursor_and_replaces_the_selection() {
    let ctx = crate::fonts::test_context();
    let id = egui::Id::new("paste_dimension");
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
    let ctx = crate::fonts::test_context();
    let id = egui::Id::new("copy_cut");
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
    let ctx = crate::fonts::test_context();
    let id = egui::Id::new("nothing_selected");
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
    let ctx = crate::fonts::test_context();
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
    let ctx = crate::fonts::test_context();
    let id = egui::Id::new("canvas_box");
    let value = RefCell::new("20".to_owned());
    let (field, canvas_menu) = (Cell::new(egui::Rect::NOTHING), Cell::new(false));
    let mut draw = |ui: &mut Ui| {
        // The viewport: a right-click on it opens its own menu.
        let canvas = ui.allocate_rect(ui.max_rect(), egui::Sense::click_and_drag());
        canvas_menu.set(canvas_menu.get() | canvas.secondary_clicked());
        let ctx = ui.ctx().clone();
        let area = Container::new(&ctx, Surface::Canvas).area(egui::Id::new("canvas_box_area"));
        area.fixed_pos(egui::pos2(60.0, 60.0)).order(egui::Order::Foreground).show(&ctx, |ui| {
            field.set(canvas_value_field(ui, id, &mut value.borrow_mut(), false).rect);
        });
    };
    menu_frame(&ctx, vec![], &mut draw);
    menu_frame(&ctx, vec![], &mut draw);
    right_click(&ctx, field.get().center(), &mut draw);
    assert!(scenario::handle_at("menu:text_paste").is_some(), "the value box's menu opens");
    assert!(!canvas_menu.get(), "the canvas does not see the right-click");
    menu_frame(&ctx, key(egui::Key::Escape), &mut draw);
    menu_frame(&ctx, vec![], &mut draw);
    right_click(&ctx, egui::pos2(250.0, 300.0), &mut draw);
    assert!(canvas_menu.get(), "a right-click beside the box still reaches the canvas");
    assert!(scenario::handle_at("menu:text_paste").is_none(), "and opens no text menu");
}

#[test]
fn esc_closes_the_text_menu_alone() {
    set_clipboard(None);
    let ctx = crate::fonts::test_context();
    let id = egui::Id::new("esc_dimension");
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
    assert!(scenario::handle_at("menu:text_paste").is_none(), "Esc closes the menu");
    assert!(!open.get(), "and the suggestions may show again");
    assert!(!owner_saw_esc.get(), "the field's owner doesn't see the Esc that closed the menu");
    assert!(ctx.memory(|m| m.has_focus(id)), "and the field keeps the focus");
    menu_frame(&ctx, key(egui::Key::Escape), &mut draw);
    assert!(owner_saw_esc.get(), "with the menu closed, Esc reaches the owner again");
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
