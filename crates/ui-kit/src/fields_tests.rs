use crate::test_support::*;
use crate::*;

use egui::Ui;
use solidcraft_ui_registry as registry;

#[test]
fn table_cell_borders_are_single_inside_rings_for_hover_focus_and_errors() {
    for dark in [false, true] {
        let t = solidcraft_theme::Tokens::themed(dark, solidcraft_theme::ColorVision::default());
        let _tokens = solidcraft_theme::scoped_tokens(t);
        for (focused, invalid) in [(false, false), (true, false), (true, true)] {
            let ctx = context();
            let id = egui::Id::unique("single_cell_ring");
            let mut value = "10 mm".to_owned();
            let rect = std::cell::Cell::new(egui::Rect::NOTHING);
            let mut draw = |ui: &mut Ui| {
                ui.set_width(280.0);
                scope(ui, |ui| {
                    rect.set(cell_editor(ui, id, &mut value, "", invalid).rect);
                });
            };
            raw_frame(&ctx, egui::RawInput::default(), &mut draw);
            if focused {
                ctx.memory_mut(|m| m.request_focus(id));
            }
            let output =
                raw_frame(&ctx, egui::RawInput { events: vec![egui::Event::PointerMoved(rect.get().center())], ..Default::default() }, &mut draw);
            let mut rings = Vec::new();
            for s in &output.shapes {
                each_shape(&s.shape, &mut |shape| {
                    if let egui::Shape::Rect(r) = shape
                        && r.stroke.width > 0.0
                    {
                        rings.push(r.clone());
                    }
                });
            }
            assert_eq!(rings.len(), 1, "dark {dark}, focus {focused}, invalid {invalid}: {rings:?}");
            assert_eq!(rings[0].rect, rect.get());
            assert_eq!(rings[0].stroke_kind, egui::StrokeKind::Inside);
            assert_eq!(rings[0].corner_radius, egui::CornerRadius::same(t.control_radius));
            assert_eq!(rings[0].stroke.width, if focused && !invalid { 1.5 } else { 1.0 });
            assert_eq!(
                rings[0].stroke.color,
                if invalid {
                    t.form_error
                } else if focused {
                    crate::selection_ink(&t)
                } else {
                    t.control_border.lerp_to_gamma(t.form_dim, 0.35)
                }
            );
        }
    }
}

#[test]
fn wide_cell_editor_has_one_inside_ring_around_the_editor_and_note() {
    let t = solidcraft_theme::Tokens::get();
    for invalid in [false, true] {
        let ctx = context();
        let id = egui::Id::unique("wide_ring");
        let mut value = "a long expression that wraps over the neighbouring cells".to_owned();
        ctx.memory_mut(|m| m.request_focus(id));
        let mut last = egui::FullOutput::default();
        for _ in 0..4 {
            last = raw_frame(&ctx, egui::RawInput::default(), |ui| {
                scope(ui, |ui| {
                    ui.set_width(100.0);
                    wide_cell_editor(ui, id, &mut value, "", invalid, 460.0, ("Live value", false));
                });
            });
        }
        let mut rings = Vec::new();
        for s in &last.shapes {
            each_shape(&s.shape, &mut |shape| {
                if let egui::Shape::Rect(r) = shape
                    && r.stroke.width > 0.0
                {
                    assert!(s.clip_rect.contains_rect(r.rect));
                    rings.push(r.clone());
                }
            });
        }
        assert_eq!(rings.len(), 1, "invalid {invalid}: {rings:?}");
        assert_eq!(rings[0].stroke_kind, egui::StrokeKind::Inside);
        assert_eq!(rings[0].corner_radius, egui::CornerRadius::same(t.control_radius));
        assert_eq!(rings[0].stroke.width, if invalid { 1.0 } else { 1.5 });
        assert!(rings[0].rect.width() > 300.0);
        assert!(text_shapes(&last).any(|text| text.galley.job.text == "Live value" && rings[0].rect.contains(text.pos)));
    }
}

#[test]
fn combo_backing_zero_preserves_stock_shapes_and_semantics() {
    let fixture = |stock: bool| {
        let ctx = crate::test_support::context();
        let mut id = egui::Id::NULL;
        let mut semantics = String::new();
        let mut output = egui::FullOutput::default();
        for pass in 0..4 {
            if pass > 0 {
                egui::Popup::open_id(&ctx, id.with("popup"));
            }
            output = crate::test_support::raw_frame(&ctx, egui::RawInput::default(), |ui| {
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
            crate::test_support::each_shape(&clipped.shape, &mut |shape| shapes.push(format!("{:?} {shape:?}", clipped.clip_rect)));
        }
        (semantics, shapes)
    };
    assert_eq!(fixture(true), fixture(false), "zero adds no shapes or slots and keeps native IDs, geometry and clip");
}

#[test]
fn every_text_input_centres_its_galley_for_mono_and_proportional_fonts() {
    for case in ["expression", "plain", "cell", "invalid", "library", "placement", "password", "multiline", "area", "message", "search"] {
        let ctx = crate::test_support::context();
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
        if galley.galley.rows.len() == 1 {
            // #23381: what must read centred in the frame is the capitals' ink, not the row box
            // around it: the font's capitals sit low in the row box (ascent exceeds cap height,
            // a descent hangs under the baseline), so the row box is placed high by the measured
            // offset and the ink lands in the middle. egui pixel-snaps glyph positions, so the
            // limit is the issue's: half a point, which is half a device pixel at this DPR.
            let ink = galley.pos.y + galley.galley.mesh_bounds.center().y;
            assert!(
                (ink - rect.center().y).abs() <= 0.5,
                "{case}: capital ink centre {ink} is {} pt off the field centre {}",
                ink - rect.center().y,
                rect.center().y
            );
        } else {
            let center = galley.pos.y + galley.galley.rect.center().y;
            assert!((center - rect.center().y).abs() < 0.01, "{case}: galley centre {center} differs from field {rect:?}");
        }
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
    let ctx = crate::test_support::context();
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

struct UnitExpressions;

impl Expressions for UnitExpressions {
    fn is_literal(&self, text: &str) -> bool {
        self.parse_length_mm(text, 1.0).is_some()
    }

    fn is_unit(&self, unit: &str) -> bool {
        ["", "m", "mm", "mil", "in", "inch", "rad", "µm"].contains(&unit.trim())
    }

    fn unit_is_prefix(&self, unit: &str) -> bool {
        let unit = if unit == "μ" { "µ" } else { unit };
        !unit.is_empty() && ["m", "mm", "mil", "in", "inch", "rad", "µm"].iter().any(|name| *name != unit && name.starts_with(unit))
    }

    fn complete(&self, _: &mut Ui, _: &egui::Response, _: &mut String) -> bool {
        false
    }

    fn unit_scale(&self, _: &str) -> (f64, &'static str) {
        (1.0, "mm")
    }

    fn length_number(&self, mm: f64, _: &str, _: u8) -> String {
        mm.to_string()
    }

    fn parse_length_mm(&self, text: &str, _: f64) -> Option<f64> {
        if let Some(number) = text.strip_suffix("mm") {
            parse_number(number)
        } else if let Some(number) = text.strip_suffix('m') {
            parse_number(number).map(|n| n * 1000.0)
        } else {
            parse_number(text)
        }
    }
}

#[test]
fn numeric_fields_publish_ambiguous_units_after_pause_or_commit() {
    for kind in ["expression", "length", "bounded_length"] {
        for finish in ["completion", "pause", "enter", "blur"] {
            let ctx = context();
            install_expressions(&ctx, std::sync::Arc::new(UnitExpressions));
            let mut text = String::new();
            let mut number = 1.0;
            let mut draw = |time, events| {
                let mut result = None;
                let output = raw_frame(&ctx, egui::RawInput { time: Some(time), events, ..Default::default() }, |ui| {
                    let (enter, response) = match kind {
                        "expression" => checked_field(ui, &mut text, "", false),
                        "length" => (false, length_field(ui, &mut number, "mm", 2)),
                        _ => (false, bounded_length_field(ui, &mut number, "mm", 2, 0.0..=5000.0, 0.5)),
                    };
                    result = Some((response.id, enter, numeric_preview_ready(ui.ctx())));
                });
                (if kind == "expression" { text.clone() } else { number.to_string() }, result.unwrap(), output)
            };
            let (_, (id, _, _), _) = draw(0.0, vec![]);
            ctx.memory_mut(|m| m.request_focus(id));
            draw(0.01, vec![]);
            select(&ctx, id, 0, usize::from(kind != "expression"));
            assert_eq!(draw(0.02, vec![egui::Event::Text("2".into())]).0, "2", "{kind}: a bare number is immediate");
            let (draft, (_, _, ready), output) = draw(0.03, vec![egui::Event::Text("m".into())]);
            assert_eq!(draft, if kind == "expression" { "2m" } else { "2000" }, "{kind}: keep the editing contract");
            assert!(!ready, "{kind}/{finish}: do not preview a unit prefix");
            assert!(text_shapes(&output).any(|s| s.galley.text() == "2m"), "{kind}: preserve the visible draft");
            assert!(!draw(0.1, vec![]).1.2, "{kind}: do not preview before the pause expires");
            let (time, events) = match finish {
                "completion" => (0.11, vec![egui::Event::Text("m".into())]),
                "pause" => (0.5, vec![]),
                "enter" => (0.11, key(egui::Key::Enter)),
                _ => {
                    ctx.memory_mut(|m| m.surrender_focus(id));
                    (0.11, vec![])
                }
            };
            let (draft, (_, enter, ready), _) = draw(time, events);
            let expected = if kind == "expression" {
                if finish == "completion" { "2mm" } else { "2m" }
            } else if finish == "completion" {
                "2"
            } else {
                "2000"
            };
            assert_eq!(draft, expected, "{kind}/{finish}");
            assert!(ready, "{kind}/{finish}: the preview is ready");
            assert_eq!(enter, kind == "expression" && finish == "enter");
        }
    }
}

#[test]
fn numeric_unit_prefix_parsing_preserves_complete_values_and_identifiers() {
    for text in ["2m", "2 m", "2mi", "2in", "2 i", "2r", "2 r", "2µ", "2 μ", "2e3m", "-2,5m", "pi m", "(2) m"] {
        assert!(crate::value_focus::pending_unit(&UnitExpressions, text), "{text}");
    }
    for text in ["", "2", "2mm", "2 mm", "2m ", "2inch", "2 rad", "width2m", "width_m", "r", "2 m + 1", "2 m)"] {
        assert!(!crate::value_focus::pending_unit(&UnitExpressions, text), "{text}");
    }
}

#[test]
fn numeric_preview_pause_restarts_after_each_edit() {
    let ctx = context();
    install_expressions(&ctx, std::sync::Arc::new(UnitExpressions));
    let mut value = "2m".to_owned();
    let mut draw = |time, typed: &str| {
        let mut result = None;
        let events = if typed.is_empty() { vec![] } else { vec![egui::Event::Text(typed.into())] };
        raw_frame(&ctx, egui::RawInput { time: Some(time), events, ..Default::default() }, |ui| {
            let ready_before = numeric_preview_ready(ui.ctx());
            let response = checked_field(ui, &mut value, "", false).1;
            result = Some((response.id, numeric_preview_ready(ui.ctx()), ready_before));
        });
        result.unwrap()
    };
    let (id, _, _) = draw(0.0, "");
    ctx.memory_mut(|m| m.request_focus(id));
    assert!(draw(0.005, "").1, "focusing an existing value does not delay its preview");
    select(&ctx, id, 0, 2);
    assert!(draw(0.01, "2").1, "a complete value is immediate");
    assert!(!draw(0.1, "m").1);
    let (_, after, before) = draw(0.45, "i");
    assert!(!before && !after, "a queued keystroke reaches the field before an expired timer previews its old text");
    let (_, after, before) = draw(0.6, "");
    assert!(!before && !after, "the second keystroke restarted the pause, including when preview runs before the fields draw");
    let (_, after, before) = draw(0.76, "");
    assert!(before && after);
}

#[test]
fn numeric_preview_enter_waits_for_a_queued_edit_in_the_same_frame() {
    let ctx = context();
    install_expressions(&ctx, std::sync::Arc::new(UnitExpressions));
    let mut value = "2m".to_owned();
    let mut draw = |time, events: Vec<egui::Event>| {
        let mut result = None;
        raw_frame(&ctx, egui::RawInput { time: Some(time), events, ..Default::default() }, |ui| {
            let ready_before = numeric_preview_ready(ui.ctx());
            let response = checked_field(ui, &mut value, "", false).1;
            result = Some((response.id, ready_before));
        });
        result.unwrap()
    };
    let enter = || egui::Event::Key {
        key: egui::Key::Enter,
        physical_key: Some(egui::Key::Enter),
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    let (id, _) = draw(0.0, vec![]);
    ctx.memory_mut(|m| m.request_focus(id));
    draw(0.005, vec![]);
    select(&ctx, id, 0, 2);
    draw(0.01, vec![egui::Event::Text("2".into())]);
    draw(0.1, vec![egui::Event::Text("m".into())]);
    let (_, before) = draw(0.12, vec![egui::Event::Text("m".into()), enter()]);
    assert!(!before, "Enter in the frame of the completing keystroke must not release the earlier `2m`");
    assert!(draw(0.13, vec![]).1, "after the Enter the finished value previews immediately");
}

#[test]
fn multiline_text_retains_focus_id_and_three_line_geometry() {
    let ctx = crate::test_support::context();
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
        let ctx = crate::test_support::context();
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
fn searchable_dropdown_default_keys_keep_enter_and_close_on_escape_without_focus() {
    for kind in ["text", "custom", "icon", "fitting"] {
        for focused in [true, false] {
            let ctx = crate::test_support::context();
            let mut filter = "tw".to_owned();
            let picked = std::cell::RefCell::new(None);
            let widget = std::cell::Cell::new(egui::Id::NULL);
            let enter_reaches_host = std::cell::Cell::new(false);
            let options = [SearchOption { value: "two", label: "Two", handle: None }];
            let mut draw = |ui: &mut Ui| {
                row(ui, "Font", |ui| {
                    let id = if kind == "icon" { "icon_search_menu" } else { "default_keys" };
                    widget.set(ui.make_persistent_id(egui::IdSalt::new(id)));
                    let value = match kind {
                        "icon" => searchable_icon_dropdown(ui, "settings", "Two", &mut filter, &options),
                        "fitting" => searchable_dropdown_fitting(ui, id, "Two", &mut filter, &options, None, false),
                        _ => searchable_dropdown(ui, id, "Two", &mut filter, &options, (kind == "custom").then_some("Use typed value"), None),
                    };
                    if value.is_some() {
                        *picked.borrow_mut() = value;
                    }
                });
                enter_reaches_host.set(ui.input(|input| input.key_pressed(egui::Key::Enter)));
            };
            frame(&ctx, vec![], &mut draw);
            let at = registry::handle_at("dialog:Font").unwrap();
            frame(&ctx, pointer(at, true), &mut draw);
            frame(&ctx, pointer(at, false), &mut draw);
            frame(&ctx, vec![], &mut draw);
            frame(&ctx, key(egui::Key::ArrowDown), &mut draw);
            frame(&ctx, key(egui::Key::Enter), &mut draw);
            assert!(picked.borrow().is_none(), "{kind}: Enter must not choose a value without opting in");
            assert!(enter_reaches_host.get(), "{kind}: Enter remains available to the enclosing dialog");
            assert!(egui::ComboBox::is_open(&ctx, widget.get()), "{kind}: Enter leaves the popup open");
            let focus = if focused { widget.get().with("search_keyboard").with("editor") } else { egui::Id::unique("elsewhere") };
            ctx.memory_mut(|memory| memory.request_focus(focus));
            frame(&ctx, key(egui::Key::Escape), &mut draw);
            assert!(!egui::ComboBox::is_open(&ctx, widget.get()), "{kind}: Escape closes the popup with editor focus={focused}");
        }
    }
}

#[test]
fn searchable_dropdown_fits_rows_and_keeps_loading_status_inside_popup() {
    let ctx = crate::test_support::context();
    let count = std::cell::Cell::new(3);
    let pending = std::cell::Cell::new(true);
    let widget = std::cell::Cell::new(egui::Id::NULL);
    let names: Vec<_> = (0..40).map(|i| format!("Family {i}")).collect();
    let mut filter = String::new();
    let mut draw = |ui: &mut Ui| {
        ui.set_width(300.0);
        row(ui, "Font", |ui| {
            widget.set(ui.make_persistent_id(egui::IdSalt::new("fitting_fonts")));
            let options: Vec<_> = names.iter().take(count.get()).map(|name| SearchOption { value: name, label: name, handle: None }).collect();
            searchable_dropdown_fitting(
                ui,
                "fitting_fonts",
                "Family 0",
                &mut filter,
                &options,
                pending.get().then_some("Looking for installed fonts…"),
                false,
            );
        });
    };
    let mut render = |events| {
        raw_frame(
            &ctx,
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 720.0))),
                focused: true,
                events,
                ..Default::default()
            },
            |ui| scope(ui, &mut draw),
        )
    };
    render(vec![]);
    let at = registry::handle_at("dialog:Font").unwrap();
    render(pointer(at, true));
    render(pointer(at, false));
    let mut measure = |rows, loading| {
        count.set(rows);
        pending.set(loading);
        let mut output = egui::FullOutput::default();
        for _ in 0..4 {
            output = render(vec![]);
        }
        let popup = ctx.read_response(widget.get().with("popup")).unwrap().rect;
        let mut status = None;
        let mut painted_text = Vec::new();
        for clipped in &output.shapes {
            each_shape(&clipped.shape, &mut |shape| {
                if let egui::Shape::Text(text) = shape {
                    painted_text.push(text.galley.job.text.clone());
                    if text.galley.job.text == "Looking for installed fonts…" {
                        let rect = text.galley.rect.translate(text.pos.to_vec2());
                        assert!(popup.expand(1.0).contains_rect(rect), "status {rect:?} outside popup {popup:?}");
                        assert!(clipped.clip_rect.contains_rect(rect), "status must not be clipped");
                        status = Some(rect);
                    }
                }
            });
        }
        assert_eq!(status.is_some(), loading, "{rows} rows in {popup:?}: {painted_text:?}");
        popup.height()
    };
    let three = measure(3, true);
    let one = measure(1, true);
    assert!(one < three - 20.0, "one row ({one}) must shrink from three ({three})");
    assert!(one < 140.0, "one match and status must not reserve an empty list: {one}");
    let loaded = measure(1, false);
    assert!(loaded < one - 5.0, "loaded list drops the status row");
    let many = measure(40, true);
    assert!(many > three + 20.0, "more rows grow the popup again");
    assert!(many < solidcraft_theme::Tokens::get().space_24 * 15.0, "long lists stay bounded and scroll: {many}");
}

#[test]
fn searchable_dropdown_keyboard_picks_the_filtered_full_value() {
    let ctx = crate::test_support::context();
    let mut filter = String::new();
    let mut picked = None;
    let options = [
        SearchOption { value: "Celtic Gaelige", label: "Celtic Gaelige", handle: Some("font:gaelige") },
        SearchOption { value: "CelticHand", label: "CelticHand", handle: Some("font:hand") },
        SearchOption { value: "Liberation Sans", label: "Liberation Sans", handle: Some("font:liberation") },
    ];
    let mut draw = |ui: &mut Ui| {
        row(ui, "Font", |ui| {
            if let Some(value) = searchable_dropdown_fitting(ui, "keyboard_fonts", "", &mut filter, &options, None, true) {
                picked = Some(value);
            }
        });
    };
    frame(&ctx, vec![], &mut draw);
    let at = registry::handle_at("dialog:Font").unwrap();
    frame(&ctx, pointer(at, true), &mut draw);
    frame(&ctx, pointer(at, false), &mut draw);
    frame(&ctx, vec![], &mut draw);
    frame(&ctx, vec![egui::Event::Text("cElTiC".into())], &mut draw);
    frame(&ctx, vec![], &mut draw);
    assert!(registry::handle_at("font:liberation").is_none());
    for key in [egui::Key::ArrowDown, egui::Key::ArrowDown, egui::Key::ArrowUp, egui::Key::ArrowDown, egui::Key::Enter] {
        frame(&ctx, vec![egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }], &mut draw);
        frame(&ctx, vec![], &mut draw);
    }
    assert_eq!(picked.as_deref(), Some("CelticHand"));
}

#[test]
fn searchable_dropdown_keyboard_starts_at_the_current_family() {
    let ctx = crate::test_support::context();
    let mut filter = String::new();
    let picked = std::cell::RefCell::new(Vec::new());
    let options = [SearchOption { value: "one", label: "One", handle: None }, SearchOption { value: "two", label: "Two", handle: None }];
    let mut draw = |ui: &mut Ui| {
        row(ui, "Font", |ui| {
            if let Some(value) = searchable_dropdown_fitting(ui, "current_font", "Two", &mut filter, &options, None, true) {
                picked.borrow_mut().push(value);
            }
        });
    };
    frame(&ctx, vec![], &mut draw);
    for keys in [vec![egui::Key::Enter], vec![egui::Key::ArrowUp, egui::Key::Enter]] {
        let at = registry::handle_at("dialog:Font").unwrap();
        frame(&ctx, pointer(at, true), &mut draw);
        frame(&ctx, pointer(at, false), &mut draw);
        frame(&ctx, vec![], &mut draw);
        for pressed in keys {
            frame(&ctx, key(pressed), &mut draw);
            frame(&ctx, vec![], &mut draw);
        }
    }
    assert_eq!(*picked.borrow(), ["two", "one"], "Enter picks the highlighted current family; Up moves from that family");
}

#[test]
fn searchable_dropdown_discovery_refreshes_current_family_until_arrow_navigation() {
    for navigated in [false, true] {
        let ctx = crate::test_support::context();
        let mut filter = String::new();
        let picked = std::cell::RefCell::new(None);
        let loaded = std::cell::Cell::new(false);
        let mut draw = |ui: &mut Ui| {
            row(ui, "Font", |ui| {
                let names = if !loaded.get() {
                    vec!["Liberation Sans"]
                } else if navigated {
                    vec!["Arial", "Liberation Sans"]
                } else {
                    vec!["Liberation Sans", "Arial"]
                };
                let options: Vec<_> = names.iter().map(|name| SearchOption { value: name, label: name, handle: None }).collect();
                if let Some(value) = searchable_dropdown_fitting(ui, "discovered_fonts", "Arial", &mut filter, &options, None, true) {
                    *picked.borrow_mut() = Some(value);
                }
            });
        };
        frame(&ctx, vec![], &mut draw);
        let at = registry::handle_at("dialog:Font").unwrap();
        frame(&ctx, pointer(at, true), &mut draw);
        frame(&ctx, pointer(at, false), &mut draw);
        frame(&ctx, vec![], &mut draw);
        if navigated {
            frame(&ctx, key(egui::Key::ArrowDown), &mut draw);
            frame(&ctx, vec![], &mut draw);
        }
        loaded.set(true);
        for _ in 0..4 {
            frame(&ctx, vec![], &mut draw);
        }
        frame(&ctx, key(egui::Key::Enter), &mut draw);
        assert_eq!(
            picked.borrow().as_deref(),
            Some(if navigated { "Liberation Sans" } else { "Arial" }),
            "discovery updates the automatic highlight and preserves an arrow choice even when rows move"
        );
    }
}

#[test]
fn searchable_dropdown_empty_match_keeps_enter_and_escape_in_the_popup() {
    let ctx = crate::test_support::context();
    let mut filter = String::new();
    let mut picked = None;
    let options = [SearchOption { value: "Liberation Sans", label: "Liberation Sans", handle: None }];
    let escaped = std::cell::Cell::new(false);
    let mut draw = |ui: &mut Ui| {
        row(ui, "Font", |ui| {
            if let Some(value) = searchable_dropdown_fitting(ui, "empty_fonts", "", &mut filter, &options, None, true) {
                picked = Some(value);
            }
        });
        escaped.set(ui.input(|input| input.key_pressed(egui::Key::Escape)));
    };
    frame(&ctx, vec![], &mut draw);
    let at = registry::handle_at("dialog:Font").unwrap();
    frame(&ctx, pointer(at, true), &mut draw);
    frame(&ctx, pointer(at, false), &mut draw);
    frame(&ctx, vec![], &mut draw);
    frame(&ctx, vec![egui::Event::Text("no such family".into())], &mut draw);
    frame(&ctx, key(egui::Key::ArrowDown), &mut draw);
    frame(&ctx, key(egui::Key::Enter), &mut draw);
    frame(&ctx, vec![], &mut draw);
    assert!(registry::handle_at("dialog:Font search").is_some());
    frame(&ctx, key(egui::Key::Escape), &mut draw);
    assert!(!escaped.get(), "Esc must not also cancel the containing dialog");
    frame(&ctx, vec![], &mut draw);
    assert!(registry::handle_at("dialog:Font search").is_none());
    assert!(picked.is_none());
}

#[test]
fn search_picker_filters_focuses_picks_and_takes_custom_values() {
    let ctx = crate::test_support::context();
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
    let ctx = crate::test_support::context();
    let values = RefCell::new(vec!["a/one", "b/two"]);
    let note: Cell<Option<&str>> = Cell::new(None);
    let picked = RefCell::new(Vec::new());
    let frame = |events: Vec<egui::Event>| {
        registry::clear_handles();
        let raw =
            egui::RawInput { events, screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 700.0))), ..Default::default() };
        ctx.run_ui(raw, |ui| {
            ui.allocate_ui(egui::vec2(160.0, 40.0), |ui| {
                crate::scope(ui, |ui| {
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
    let ctx = crate::test_support::context();
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
    let ctx = crate::test_support::context();
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
    let ctx = crate::test_support::context();
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
fn secret_field_retains_focus_and_masks_paint_and_accessibility_events() {
    let ctx = crate::test_support::context();
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
    let ctx = crate::test_support::context();
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
        let ctx = crate::test_support::context();
        let width = |value: &str| {
            let mut value = value.to_owned();
            let mut rect = egui::Rect::NOTHING;
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                egui::Area::new(egui::Id::unique("grow")).fixed_pos(egui::pos2(10.0, 10.0)).show(ui.ctx(), |ui| {
                    ui.horizontal(|ui| {
                        let id = ui.scope_id().with(case);
                        rect = match case {
                            "canvas" => canvas_value_field(ui, id, &mut value, false, "").rect,
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

/// A canvas value field's hint is shown in the field while it is empty, and not beside a value.
#[test]
fn a_canvas_value_field_shows_its_hint_only_while_it_is_empty() {
    fn walk(shape: &egui::Shape, out: &mut Vec<String>) {
        match shape {
            egui::Shape::Text(t) => out.push(t.galley.job.text.clone()),
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let ctx = crate::test_support::context();
    let painted = |value: &str, hint: &str| {
        let mut value = value.to_owned();
        let mut texts = Vec::new();
        // An area paints nothing in the frame it is first sized in.
        for _ in 0..2 {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                egui::Area::new(egui::Id::unique("hinted")).fixed_pos(egui::pos2(10.0, 10.0)).show(ui.ctx(), |ui| {
                    canvas_value_field(ui, ui.scope_id().with("canvas"), &mut value, false, hint);
                });
            });
            output.textures_delta.clear();
            texts.clear();
            output.shapes.iter().for_each(|s| walk(&s.shape, &mut texts));
        }
        texts
    };
    assert!(painted("", "All").iter().any(|t| t == "All"), "an empty field shows its hint");
    assert!(!painted("12 mm", "All").iter().any(|t| t == "All"), "a value takes the hint's place");
    assert!(!painted("", "").iter().any(|t| t == "All"), "no hint, no text");
}

#[test]
fn placement_field_keeps_focus_for_owner_validation_on_enter() {
    let ctx = crate::test_support::context();
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
    let ctx = crate::test_support::context();
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
    let ctx = crate::test_support::context();
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

/// The same quantity editor is used by solid and sketch pattern dialogs. Up/Down steps
/// its value only when focused, leaving limits and expression values intact.
#[test]
fn count_keyboard_arrows_step_only_a_focused_count_within_limits() {
    for (initial, arrow, expected) in [
        ("5", egui::Key::ArrowUp, "6"),
        ("5", egui::Key::ArrowDown, "4"),
        ("1", egui::Key::ArrowDown, "1"),
        ("10", egui::Key::ArrowUp, "10"),
        ("n", egui::Key::ArrowUp, "n + 1"),
    ] {
        let ctx = crate::test_support::context();
        let eval = |text: &str| match text.trim() {
            "n" => Some(4.0),
            "n + 1" => Some(5.0),
            text => text.parse::<f64>().ok(),
        };
        let mut count = initial.to_string();
        let mut draw = |ui: &mut Ui| {
            count_row(ui, "Instances", &mut count, &Count { min: 1, max: 10, eval: &eval });
        };
        frame(&ctx, vec![], &mut draw);
        // A key outside the field does nothing, even with the dialog visible.
        frame(&ctx, key(egui::Key::ArrowUp), &mut draw);
        let field = registry::rect_of("dialog:widget:Instances").expect("count field has a rect");
        let at = field.left_center() + egui::vec2(16.0, 0.0);
        frame(&ctx, pointer(at, true), &mut draw);
        frame(&ctx, pointer(at, false), &mut draw);
        frame(&ctx, key(arrow), &mut draw);
        assert_eq!(count, expected, "{initial} with {arrow:?}");
    }
}

/// A count's step arrows stand inside the editor, which asks for the text cursor across its
/// whole rect, so the arrows showed an I-beam and read as part of the text. They are not text:
/// the pointer over one is the ordinary cursor, whether it can step or not, and the number
/// beside them is still typed.
#[test]
fn count_step_arrows_do_not_take_the_text_cursor() {
    for value in ["5", "1"] {
        let ctx = crate::test_support::context();
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
    crate::test_support::raw_frame(ctx, input, |ui| {
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
        let ctx = crate::test_support::context();
        let value = RefCell::new("12 mm".to_owned());
        let rect = Cell::new(egui::Rect::NOTHING);
        let mut draw = |ui: &mut Ui| {
            let id = ui.scope_id().with(case);
            let value = &mut *value.borrow_mut();
            rect.set(match case {
                "expression" | "plain" => super::editor(ui, value, "", case == "expression").2.rect,
                "cell" => cell_editor(ui, id, value, "", false).rect,
                "placement" => placement_field(ui, id, value, "").rect,
                "canvas" => canvas_value_field(ui, id, value, false, "").rect,
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
    let ctx = crate::test_support::context();
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
    assert_eq!(*value.borrow(), "4 mm ", "Paste replaces the selection; each line ending becomes one space");
    set_clipboard(None);
}

#[test]
fn single_line_paste_normalizes_each_line_ending_to_one_space() {
    for ending in ["\r\n", "\n", "\r"] {
        let clip = format!("尺寸{ending}🙂");
        let mut text = String::new();
        let inserted = egui::TextBuffer::insert_text(&mut super::SingleLineBuffer(&mut text), &clip, egui::text::CharIndex(0));
        assert_eq!(text, "尺寸 🙂", "keyboard paste with {ending:?}");
        assert_eq!(inserted, 4);

        let mut text = String::new();
        let done = text_menu::apply(&mut text, None, Edit::Paste, Some(&clip), Field::LINE);
        assert_eq!(text, "尺寸 🙂", "menu paste with {ending:?}");
        assert_eq!(done.range.primary.index.0, 4);
        assert!(done.changed);
    }
}

#[test]
fn copy_and_cut_put_the_selection_on_the_clipboard() {
    set_clipboard(None);
    let ctx = crate::test_support::context();
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
    let ctx = crate::test_support::context();
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
    let ctx = crate::test_support::context();
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
    let ctx = crate::test_support::context();
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
            field.set(canvas_value_field(ui, id, &mut value.borrow_mut(), false, "").rect);
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
fn keyboard_text_menu_refreshes_clipboard_once_per_opening() {
    let ctx = crate::test_support::context();
    let id = egui::Id::unique("keyboard_clipboard");
    let value = RefCell::new(String::new());
    let mut draw = |ui: &mut Ui| {
        cell_editor(ui, id, &mut value.borrow_mut(), "", false);
    };
    menu_frame(&ctx, vec![], &mut draw);
    ctx.memory_mut(|memory| memory.request_focus(id));
    for text in ["first", "second"] {
        set_clipboard(Some(text));
        select(&ctx, id, 0, value.borrow().chars().count());
        let modifiers = egui::Modifiers::SHIFT;
        let events = vec![
            egui::Event::ModifiersChanged(modifiers),
            egui::Event::Key { key: egui::Key::F10, physical_key: None, pressed: true, repeat: false, modifiers },
            egui::Event::Key { key: egui::Key::F10, physical_key: None, pressed: false, repeat: false, modifiers },
        ];
        registry::clear_handles();
        crate::test_support::raw_frame(
            &ctx,
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(320.0, 400.0))),
                events,
                focused: true,
                ..Default::default()
            },
            |ui| {
                ui.set_width(280.0);
                scope(ui, draw);
            },
        );
        // The open menu uses its snapshot, even if another app changes the clipboard.
        set_clipboard(Some("changed while open"));
        menu_frame(&ctx, vec![egui::Event::ModifiersChanged(egui::Modifiers::NONE)], &mut draw);
        pick(&ctx, "text_paste", &mut draw);
        assert_eq!(*value.borrow(), text, "each keyboard opening refreshes the snapshot");
    }
}

#[test]
fn esc_closes_the_text_menu_alone() {
    set_clipboard(None);
    let ctx = crate::test_support::context();
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
            let ctx = crate::test_support::context();
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
