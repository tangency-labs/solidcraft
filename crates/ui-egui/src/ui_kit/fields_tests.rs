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
