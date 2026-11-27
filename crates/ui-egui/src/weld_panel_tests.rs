use super::*;
use egui::{Context, Event, Modifiers, PointerButton, Pos2, RawInput, vec2};

fn render(ctx: &Context, state: &mut WeldPanelState, dark: bool, events: Vec<Event>) -> (WeldPanelEvent, Measurements) {
    solidcraft_theme::set_dark(dark);
    solidcraft_theme::apply(ctx);
    let t = Tokens::get();
    let mut result = None;
    let mut output =
        ctx.run_ui(RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1100.0, 3600.0))), events, ..Default::default() }, |ui| {
            ui.set_width(344.0);
            result = Some(sections::show_measured(ui, state, &t));
        });
    output.textures_delta.clear();
    assert!(!output.shapes.is_empty());
    result.expect("the frame rendered its card")
}

fn context() -> Context {
    crate::i18n::install();
    let ctx = Context::default();
    solidcraft_look::install(&ctx);
    ctx
}

fn section(m: &Measurements, name: &str) -> Rect {
    m.sections.iter().find(|(key, _)| *key == name).expect("section exists").1
}

fn close(actual: f32, expected: f32, tolerance: f32) {
    assert!((actual - expected).abs() <= tolerance, "{actual} differs from {expected} by more than {tolerance}");
}

#[test]
fn weld_panel_fillet_board_section_heights_and_order() {
    let ctx = context();
    let mut state = WeldPanelState::specimen(WeldType::Fillet, false);
    let (_, m) = render(&ctx, &mut state, false, vec![]);
    let board = ["type", "where", "size", "advanced", "face", "extent", "process", "look", "notices"];
    println!(
        "Weld fillet measured heights: {:?}; total {}",
        m.sections.iter().map(|(key, rect)| (*key, rect.height())).collect::<Vec<_>>(),
        m.card.expect("card").height()
    );
    assert_eq!(m.sections.iter().map(|(key, _)| *key).collect::<Vec<_>>(), board.to_vec());
    close(m.card.expect("card").width(), 344.0, 0.5);
    for key in board {
        assert!(section(&m, key).height() > 0.0);
    }
    assert!(section(&m, "size").contains_rect(section(&m, "advanced")));
    let outer: Vec<_> = m.sections.iter().filter(|(key, _)| *key != "advanced").collect();
    for pair in outer.windows(2) {
        close(pair[0].1.bottom(), pair[1].1.top(), 0.5);
    }
    close(m.card.expect("card").height(), 3600.0, 0.5);
}

#[test]
fn weld_panel_size_columns_align_centre_and_have_seven_point_gaps() {
    for typ in [WeldType::Fillet, WeldType::Butt, WeldType::Spot, WeldType::Plug, WeldType::Stud, WeldType::Surfacing] {
        for advanced in [false, true] {
            let ctx = context();
            let mut state = WeldPanelState::specimen(typ, advanced);
            let (_, m) = render(&ctx, &mut state, false, vec![]);
            let figure = m.figure.expect("figure");
            let content = m.content.expect("size content");
            let column = m.field_column.expect("field column");
            close(figure.width(), 164.0, 0.5);
            close(figure.left(), section(&m, "size").left() + 14.0, 0.5);
            close(figure.left(), content.left(), 0.5);
            close(column.left() - figure.right(), 12.0, 0.5);
            close(column.center().y, figure.center().y, 2.0);
            for field in &m.fields {
                close(field.left(), column.left(), 0.5);
                close(field.right(), content.right(), 0.5);
                assert!(!field.intersects(figure));
                assert!(column.expand(0.5).contains_rect(*field));
            }
            for fields in m.fields.windows(2) {
                close(fields[1].top() - fields[0].bottom(), 7.0, 0.5);
            }
        }
    }
}

#[test]
fn weld_panel_widgets_do_not_overlap_or_escape_the_card() {
    for typ in [WeldType::Fillet, WeldType::Butt, WeldType::Spot, WeldType::Plug, WeldType::Stud, WeldType::Surfacing, WeldType::Seam] {
        let ctx = context();
        let mut state = WeldPanelState::specimen(typ, true);
        let (_, m) = render(&ctx, &mut state, false, vec![]);
        let card = m.card.expect("card");
        for (index, widget) in m.widgets.iter().enumerate() {
            assert!(card.expand(0.5).contains_rect(*widget), "{typ:?}: {widget:?} outside {card:?}");
            for other in m.widgets.iter().skip(index + 1) {
                assert!(!widget.shrink(0.5).intersects(other.shrink(0.5)), "{typ:?}: {widget:?} overlaps {other:?}");
            }
        }
    }
}

#[test]
fn weld_panel_spot_unused_sections_are_one_grey_line() {
    let ctx = context();
    let mut state = WeldPanelState::specimen(WeldType::Spot, false);
    let (_, m) = render(&ctx, &mut state, false, vec![]);
    close(section(&m, "face").height(), 35.0, 2.0);
    close(section(&m, "extent").height(), 35.0, 2.0);
}

#[test]
fn weld_panel_type_popup_keeps_card_height() {
    let ctx = context();
    let mut state = WeldPanelState::specimen(WeldType::Fillet, false);
    let (_, first) = render(&ctx, &mut state, false, vec![]);
    state.type_picker_open = true;
    let (_, open) = render(&ctx, &mut state, false, vec![]);
    close(first.card.expect("card").height(), open.card.expect("card").height(), 0.5);
    assert!(state.type_picker_open);
}

#[test]
fn weld_panel_all_types_and_notices_paint_in_both_themes() {
    for dark in [false, true] {
        for info in TYPES {
            let ctx = context();
            let mut state = WeldPanelState::specimen(info.weld_type, true);
            state.notices = WeldNotice::all_samples();
            let (_, m) = render(&ctx, &mut state, dark, vec![]);
            close(m.card.expect("card").width(), 344.0, 0.5);
            assert!(m.card.expect("card").is_finite());
        }
    }
}

fn click(ctx: &Context, state: &mut WeldPanelState, at: Pos2) -> WeldPanelEvent {
    render(
        ctx,
        state,
        false,
        vec![Event::PointerMoved(at), Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE }],
    );
    render(ctx, state, false, vec![Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE }]).0
}

#[test]
fn weld_panel_actions_and_type_selection_emit_events_without_commands() {
    let ctx = context();
    let mut state = WeldPanelState::specimen(WeldType::Fillet, false);
    let (_, m) = render(&ctx, &mut state, false, vec![]);
    let symbol = m.define.expect("Define symbol").center();
    assert_eq!(click(&ctx, &mut state, symbol), WeldPanelEvent::DefineSymbol);
    let cancel = m.cancel.expect("Cancel").center();
    assert_eq!(click(&ctx, &mut state, cancel), WeldPanelEvent::Cancel);
    let done = m.ok.expect("OK").center();
    assert_eq!(click(&ctx, &mut state, done), WeldPanelEvent::Done);
    let groove = m.type_choices[1].button.center();
    assert_eq!(click(&ctx, &mut state, groove), WeldPanelEvent::TypePicked(WeldType::Butt));
    assert_eq!(state.weld_type, WeldType::Butt);
    assert_eq!(state.size.a, "3.0");
    assert_eq!(state.picks.line.len(), 2);
}

#[test]
fn weld_panel_type_table_covers_the_board_and_has_registered_icons() {
    crate::i18n::install();
    assert_eq!(TYPES.len(), 9);
    assert_eq!(TYPES.iter().map(|info| info.label.trim_start_matches("ui.weld.type.")).collect::<Vec<_>>(), solidcraft_engine::doc::weld::TYPES);
    assert_eq!(TYPES.iter().filter(|info| info.visible).count(), 4);
    assert_eq!(TYPES.iter().filter(|info| info.look_tbd).count(), 0);
    for info in TYPES {
        assert_eq!(info.weld_type.info().icon, info.icon);
        assert_ne!(tr(info.label), info.label);
        assert_ne!(crate::icon_svg::resolve(info.icon).master, crate::icon_svg::PLACEHOLDER);
    }
}

#[test]
fn weld_panel_notice_buttons_emit_the_original_notice_index() {
    let ctx = context();
    let mut state = WeldPanelState::specimen(WeldType::Fillet, false);
    state.notices = WeldNotice::all_samples();
    let (_, m) = render(&ctx, &mut state, false, vec![]);
    let fit = m.notice_actions.iter().find(|(index, _)| *index == 1).expect("fit button").1.center();
    let lost = m.notice_actions.iter().find(|(index, _)| *index == 5).expect("lost reference button").1.center();
    assert_eq!(click(&ctx, &mut state, fit), WeldPanelEvent::UseFit(1));
    assert_eq!(click(&ctx, &mut state, lost), WeldPanelEvent::PickAgain(5));
    assert_eq!(state.size.a, "3.0", "only a future engine adapter applies a suggested fit");
}

#[test]
fn weld_panel_new_bead_and_advanced_keep_the_local_edits() {
    let ctx = context();
    let mut state = WeldPanelState::specimen(WeldType::Fillet, false);
    state.size.a = "plate_thickness / 2".into();
    let (_, m) = render(&ctx, &mut state, false, vec![]);
    assert_eq!(
        click(&ctx, &mut state, m.apply.expect("New weld bead keeps the dialog:apply target in the left slot").center()),
        WeldPanelEvent::NewBead
    );
    click(&ctx, &mut state, section(&m, "advanced").center());
    assert!(state.advanced_open);
    let (_, advanced) = render(&ctx, &mut state, false, vec![]);
    assert_eq!(advanced.fields.len(), 1);
    assert_eq!(state.size.a, "plate_thickness / 2");
}

#[test]
fn weld_panel_advanced_header_is_frameless_and_stays_inside_the_card() {
    fn inspect(shape: &egui::Shape, card: Rect, row: Rect, clip: Rect, bounds: &mut Rect, text: &mut Vec<(String, Rect)>) {
        if let egui::Shape::Vec(shapes) = shape {
            for shape in shapes {
                inspect(shape, card, row, clip, bounds, text);
            }
            return;
        }
        // The shared floating container intentionally casts its themed shadow outside
        // the card. Only that exact shadow is excluded from the surface/content bounds.
        let t = Tokens::get();
        if matches!(shape, egui::Shape::Rect(r) if *r == t.shell_shadow().as_shape(card, t.radius_xl)) {
            return;
        }
        if matches!(shape, egui::Shape::Text(label) if label.galley.num_vertices == 0) {
            return;
        }
        let painted = shape.visual_bounding_rect().intersect(clip);
        if painted.is_positive() {
            assert!(card.expand(0.5).contains_rect(painted), "paint {painted:?} escapes card {card:?}: {shape:?}");
            *bounds = bounds.union(painted);
        }
        if let egui::Shape::Rect(frame) = shape {
            assert!(!row.contains(frame.rect.center()), "Advanced must not paint a rectangular button surface: {frame:?}");
        }
        if let egui::Shape::Text(label) = shape {
            let rect = label.galley.rect.translate(label.pos.to_vec2());
            if row.contains(rect.center()) {
                assert!(row.contains_rect(rect));
                text.push((label.galley.job.text.clone(), rect));
            }
        }
    }

    for dark in [false, true] {
        for open in [false, true] {
            let ctx = context();
            solidcraft_theme::set_dark(dark);
            solidcraft_theme::apply(&ctx);
            let mut state = WeldPanelState::specimen(WeldType::Fillet, open);
            let mut measured = None;
            let mut output =
                ctx.run_ui(RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1100.0, 3600.0))), ..Default::default() }, |ui| {
                    ui.set_width(344.0);
                    measured = Some(sections::show_measured(ui, &mut state, &Tokens::get()).1);
                });
            output.textures_delta.clear();
            let m = measured.expect("card");
            let card = m.card.expect("card rect");
            let row = section(&m, "advanced");
            let mut bounds = Rect::NOTHING;
            let mut text = Vec::new();
            for shape in &output.shapes {
                inspect(&shape.shape, card, row, shape.clip_rect, &mut bounds, &mut text);
            }
            assert_eq!(text.iter().map(|(label, _)| label.as_str()).collect::<Vec<_>>(), [tr("ui.weld.section.advanced")]);
            close(row.height(), 36.0, 0.5);
            println!("dark={dark}, open={open}: card {card:?}, painted {bounds:?}, Advanced {row:?}, text {text:?}");
        }
    }
}

#[test]
fn weld_panel_whole_advanced_row_toggles_from_either_state() {
    for open in [false, true] {
        // Include the leading padding, chevron, title, hint and trailing padding.
        for offset in [2.0, 20.0, 60.0, 180.0, 300.0] {
            let ctx = context();
            let mut state = WeldPanelState::specimen(WeldType::Fillet, open);
            let (_, m) = render(&ctx, &mut state, false, vec![]);
            let row = section(&m, "advanced");
            click(&ctx, &mut state, egui::pos2(row.left() + offset, row.center().y));
            assert_eq!(state.advanced_open, !open, "click {offset} in open={open} header");
        }
    }
}

#[test]
fn weld_panel_type_labels_have_their_own_two_line_slot_in_both_themes() {
    for dark in [false, true] {
        let ctx = context();
        let mut state = WeldPanelState::specimen(WeldType::Fillet, false);
        let (_, m) = render(&ctx, &mut state, dark, vec![]);
        assert_eq!(m.type_choices.len(), 4);
        for choice in &m.type_choices {
            close(choice.button.height(), 66.0, 0.5);
            close(choice.icon.width(), 24.0, 0.5);
            close(choice.icon.height(), 24.0, 0.5);
            close(choice.icon.top() - choice.button.top(), 8.0, 0.5);
            assert!(choice.label.top() >= choice.icon.bottom() + 2.0 - 0.5);
            assert!(choice.button.contains_rect(choice.label));
            close(choice.label.center().x, choice.button.center().x, 0.5);
            assert!(choice.lines <= 2);
        }
        let groove = &m.type_choices[1];
        assert_eq!(groove.lines, 2, "exercise the actual wrapped Butt / groove caption");
        println!(
            "dark={dark}: type button {}, icon {}, text {} lines, gap {}",
            groove.button.height(),
            groove.icon.width(),
            groove.lines,
            groove.label.top() - groove.icon.bottom()
        );
    }
}

#[test]
fn weld_panel_more_types_paints_a_full_width_dashed_outline() {
    for dark in [false, true] {
        let ctx = context();
        solidcraft_theme::set_dark(dark);
        solidcraft_theme::apply(&ctx);
        let t = Tokens::get();
        let mut state = WeldPanelState::specimen(WeldType::Fillet, false);
        let mut measured = None;
        let mut output =
            ctx.run_ui(RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1100.0, 3600.0))), ..Default::default() }, |ui| {
                ui.set_width(344.0);
                measured = Some(sections::show_measured(ui, &mut state, &t).1);
            });
        output.textures_delta.clear();
        let m = measured.expect("card");
        let button = m.more_button.expect("More types");
        close(button.width(), 314.0, 0.5);
        close(button.height(), 30.0, 0.5);
        let dashes: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|shape| match shape.shape {
                egui::Shape::LineSegment { points, stroke } if stroke.color == t.control_border && points.iter().all(|p| button.contains(*p)) => {
                    Some(points)
                }
                _ => None,
            })
            .collect();
        assert!(dashes.len() > 20, "expected separate dash strokes, got {}", dashes.len());
        assert!(dashes.iter().all(|pair| pair[0].distance(pair[1]) <= 3.1));
        assert!(
            !output
                .shapes
                .iter()
                .any(|shape| matches!(&shape.shape, egui::Shape::Rect(r) if r.rect == button && r.stroke.width > 0.0 && r.stroke.color.a() > 0)),
            "no solid button border underneath the dashes"
        );
        println!("dark={dark}: More types {} x {}, {} separate dashes", button.width(), button.height(), dashes.len());
    }
}

#[test]
fn weld_panel_horizontal_host_keeps_width_and_vertical_section_order() {
    for dark in [false, true] {
        let ctx = context();
        let mut state = WeldPanelState::specimen(WeldType::Fillet, false);
        let (_, baseline) = render(&ctx, &mut state, dark, vec![]);
        let mut cards = Vec::new();
        let mut output =
            ctx.run_ui(RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1600.0, 3600.0))), ..Default::default() }, |ui| {
                ui.spacing_mut().item_spacing.x = 16.0;
                ui.horizontal_top(|ui| {
                    for index in 0..4 {
                        ui.push_id(index, |ui| cards.push(sections::show_measured(ui, &mut state, &Tokens::get()).1));
                    }
                });
            });
        output.textures_delta.clear();
        assert_eq!(cards.len(), 4);
        for card in &cards {
            close(card.card.expect("card").width(), 344.0, 0.5);
            close(card.card.expect("card").height(), baseline.card.expect("baseline").height(), 0.5);
            assert_eq!(
                card.sections.iter().map(|(key, _)| *key).collect::<Vec<_>>(),
                baseline.sections.iter().map(|(key, _)| *key).collect::<Vec<_>>()
            );
            let outer: Vec<_> = card.sections.iter().filter(|(key, _)| *key != "advanced").collect();
            for pair in outer.windows(2) {
                close(pair[0].1.bottom(), pair[1].1.top(), 0.5);
            }
        }
        for pair in cards.windows(2) {
            close(pair[1].card.expect("next").left() - pair[0].card.expect("previous").right(), 16.0, 0.5);
        }
        println!(
            "dark={dark}: four horizontal cards, width {}, height {}, same section order",
            cards[0].card.expect("card").width(),
            cards[0].card.expect("card").height()
        );
    }
}

fn symbol_frame(state: &WeldPanelState, dark: bool) -> (bool, Rect, Vec<egui::Shape>, Vec<String>) {
    let ctx = context();
    solidcraft_theme::set_dark(dark);
    solidcraft_theme::apply(&ctx);
    // The pinned symbol row is wide: the symbol keeps one scale and is centred in it.
    let rect =
        Rect::from_min_size(egui::pos2(20.0, 20.0), vec2(320.0, sections::chrome::symbol_band_height(&Tokens::get()) - 2.0 * Tokens::get().space_8));
    let mut defined = false;
    let mut expected_labels = Vec::new();
    let mut output = ctx.run_ui(RawInput::default(), |ui| {
        let t = Tokens::get();
        if let Ok(record) = symbol::record(state) {
            expected_labels = symbol::drawing(ui, &record, state, &t).expect("valid symbol").text.into_iter().map(|text| text.text).collect();
        }
        defined = symbol::paint(ui, rect, state, &t);
    });
    output.textures_delta.clear();
    let shapes = output.shapes.into_iter().map(|shape| shape.shape).filter(|shape| shape.visual_bounding_rect().is_positive()).collect();
    (defined, rect, shapes, expected_labels)
}

#[test]
fn weld_panel_left_new_bead_button_fits_narrow_cards_in_both_themes() {
    assert!(!solidcraft_look::icons::SET.resolve("weld_new_bead").placeholder);
    for dark in [false, true] {
        for width in [344.0, 240.0, 180.0] {
            let ctx = context();
            solidcraft_theme::set_dark(dark);
            solidcraft_theme::apply(&ctx);
            let mut state = WeldPanelState::default();
            let mut measured = None;
            let mut output = ctx.run_ui(RawInput::default(), |ui| {
                ui.set_width(width);
                measured = Some(sections::show_contents_measured(ui, &mut state, &Tokens::get(), 700.0, |_| {}).1);
            });
            output.textures_delta.clear();
            let m = measured.unwrap();
            let footer = m.footer.unwrap();
            let apply = m.apply.unwrap();
            let cancel = m.cancel.unwrap();
            let ok = m.ok.unwrap();
            assert!(footer.contains_rect(apply) && footer.contains_rect(cancel) && footer.contains_rect(ok), "dark={dark}, width={width}");
            close(apply.left(), footer.left() + Tokens::get().space_12, 0.5);
            assert!(apply.right() <= cancel.left() && cancel.right() <= ok.left(), "dark={dark}, width={width}");
            assert_eq!(solidcraft_ui_registry::count_of("dialog:apply:enabled"), Some(1.0));
        }
    }
}

#[test]
fn weld_panel_700_point_card_pins_chrome_while_only_body_scrolls() {
    for dark in [false, true] {
        let ctx = context();
        solidcraft_theme::set_dark(dark);
        solidcraft_theme::apply(&ctx);
        let mut state = WeldPanelState::specimen(WeldType::Fillet, true);
        let mut frame = |events| {
            let mut measured = None;
            let mut output = ctx.run_ui(
                RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1100.0, 900.0))), events, ..Default::default() },
                |ui| {
                    ui.set_width(344.0);
                    ui.set_max_height(700.0);
                    measured = Some(sections::show_measured(ui, &mut state, &Tokens::get()).1);
                },
            );
            output.textures_delta.clear();
            measured.expect("card")
        };
        let before = frame(vec![]);
        let body = before.body.expect("body viewport");
        frame(vec![
            Event::PointerMoved(body.center()),
            Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                source: egui::MouseWheelSource::Unknown,
                delta: vec2(0.0, -400.0),
                modifiers: Modifiers::NONE,
                phase: egui::TouchPhase::Move,
            },
        ]);
        // ScrollArea consumes the wheel after laying out its current content. Measure the
        // following frame, when the retained body offset has been applied to the sections.
        let after = frame(vec![]);
        let card = after.card.expect("card");
        let header = after.header.expect("header");
        let footer = after.footer.expect("footer");
        let preview = after.preview.expect("sticky preview");
        close(card.height(), 700.0, 0.5);
        close(header.top(), card.top() + 1.0, 0.5);
        assert_eq!(before.header, after.header);
        assert_eq!(before.footer, after.footer);
        assert!(section(&after, "type").top() < section(&before, "type").top(), "body scroll moved its content");
        close(footer.bottom(), card.bottom() - 1.0, 0.5);
        // Two pinned rows: the symbol row sits directly above the shared Cancel / OK bar.
        assert!(preview.bottom() <= footer.top() + 0.5 && preview.top() >= body.bottom() - 0.5);
        assert!(preview.width() > footer.width() * 0.8, "the symbol row is wide");
        assert!(body.bottom() <= preview.top());
        assert!(header.bottom() <= body.top());
        assert_eq!(after.define, after.preview);
        let apply = after.apply.expect("New weld bead button");
        assert!(footer.contains_rect(apply), "New weld bead stays in the pinned bottom bar");
        assert!(apply.right() <= after.cancel.expect("Cancel").left());
        let buttons = [after.cancel.expect("Cancel"), after.ok.expect("OK")];
        for button in buttons {
            assert!(footer.contains_rect(button));
            close(button.height(), Tokens::get().form_button_height, 0.5);
        }
        assert!(!buttons[0].intersects(buttons[1]));
        let buttons = [after.define.expect("Define symbol"), buttons[0], buttons[1]];
        // Compare the actual shared footer, rather than duplicating its width formula.
        let mut reference = None;
        let mut output = ctx.run_ui(RawInput::default(), |ui| {
            ui.set_width(342.0);
            crate::ui_kit::footer(ui, "OK", tr("ui.weld.cancel"), true, crate::ui_kit::Footer::default());
            reference = Some((solidcraft_ui_registry::rect_of("dialog:cancel").unwrap(), solidcraft_ui_registry::rect_of("dialog:ok").unwrap()));
        });
        output.textures_delta.clear();
        let (cancel, ok) = reference.unwrap();
        assert_eq!(buttons[1].size(), cancel.size());
        assert_eq!(buttons[2].size(), ok.size());
        println!("dark={dark}: card {card:?}; header {header:?}; body {body:?}; preview {preview:?}; footer {footer:?}; buttons {buttons:?}");
    }
}

#[test]
fn weld_symbol_fillet_a3_paints_geometry_and_draw_labels_inside_preview_in_both_themes() {
    for dark in [false, true] {
        let state = WeldPanelState::default();
        let (defined, rect, shapes, expected_labels) = symbol_frame(&state, dark);
        assert!(defined);
        assert_eq!(expected_labels, ["a3", "135"]);
        assert!(shapes.iter().any(|shape| matches!(shape, egui::Shape::Path(_))));
        let labels: Vec<_> = shapes
            .iter()
            .filter_map(|shape| match shape {
                egui::Shape::Text(text) => Some(text.galley.job.text.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(labels, expected_labels, "paint the exact text returned by Weld::draw");
        let bounds = shapes.iter().fold(Rect::NOTHING, |bounds, shape| {
            let painted = shape.visual_bounding_rect();
            assert!(rect.contains_rect(painted), "{painted:?} outside {rect:?}");
            bounds.union(painted)
        });
        close(bounds.center().x, rect.center().x, 0.5);
        close(bounds.center().y, rect.center().y, 0.5);
    }
}

#[test]
fn weld_symbol_unsupported_type_paints_the_dimmed_icon_and_undefined_hint() {
    for dark in [false, true] {
        let state = WeldPanelState { weld_type: WeldType::Spot, ..Default::default() };
        let (defined, rect, shapes, labels) = symbol_frame(&state, dark);
        assert!(!defined);
        assert!(labels.is_empty());
        let mesh = shapes
            .iter()
            .find_map(|shape| match shape {
                egui::Shape::Mesh(mesh) => Some(mesh),
                _ => None,
            })
            .expect("icon texture mesh");
        assert!(rect.contains_rect(mesh.calc_bounds()));
        assert!(!mesh.vertices.is_empty());
        assert!(mesh.vertices.iter().all(|vertex| (1..255).contains(&vertex.color.a())), "placeholder is visible and dimmed");
    }
}

#[test]
fn weld_symbol_invalid_sizes_paint_basic_geometry_without_panicking() {
    for typ in [WeldType::Fillet, WeldType::Butt] {
        for value in ["", "0", "-3", "NaN", "inf", "100001", "0.0000001", "plate / 2"] {
            if typ == WeldType::Butt && value.is_empty() {
                continue;
            } // Full penetration is valid.
            let mut state = WeldPanelState { weld_type: typ, partial_penetration: typ == WeldType::Butt, ..Default::default() };
            state.size.a = value.into();
            state.size.s = value.into();
            let (defined, rect, shapes, labels) = symbol_frame(&state, false);
            assert!(!defined, "{typ:?} size {value}");
            assert!(labels.is_empty());
            assert!(shapes.iter().any(
                |shape| matches!(shape, egui::Shape::Path(path) if path.stroke.color == egui::epaint::ColorMode::Solid(Tokens::get().form_dim))
            ));
            assert!(shapes.iter().all(|shape| rect.contains_rect(shape.visual_bounding_rect())));
        }
    }
}

#[test]
fn weld_symbol_flipped_side_moves_the_weld_to_the_other_side_of_the_line() {
    let mut state = WeldPanelState::specimen(WeldType::Fillet, false);
    state.side = 1;
    let record = symbol::record(&state).expect("record");
    assert!(record.arrow_side.is_none() && record.other_side.is_some());
    state.side = 0;
    let record = symbol::record(&state).expect("record");
    assert!(record.arrow_side.is_some() && record.other_side.is_none());
}

#[test]
fn weld_symbol_record_maps_size_contour_sides_flags_and_process() {
    use solidcraft_engine::geom::weld::{Contour, Kind, Size};
    crate::i18n::install();
    let mut state = WeldPanelState {
        size_by_leg: true,
        both_sides: true,
        all_around: true,
        field_weld: true,
        face: Face::Flat,
        process: "141".into(),
        ..Default::default()
    };
    let record = symbol::record(&state).expect("valid fillet");
    let side = record.arrow_side.as_ref().expect("arrow side");
    assert_eq!(side.kind, Kind::Fillet);
    assert_eq!(side.size, Some(Size::Leg(4.2)));
    assert_eq!(side.contour, Some(Contour::Flush));
    assert_eq!(record.arrow_side, record.other_side);
    assert!(record.all_round && record.field);
    assert_eq!(record.tail.as_deref(), Some("141"));
    state.weld_type = WeldType::Butt;
    state.size.s = "2.50".into();
    state.partial_penetration = true;
    state.face = Face::Concave;
    state.process = "135".into();
    let record = symbol::record(&state).expect("valid butt");
    let side = record.arrow_side.expect("arrow side");
    assert_eq!(side.kind, Kind::SingleVButt);
    assert_eq!(side.size, Some(Size::ButtPenetration(2.5)));
    assert_eq!(side.contour, Some(Contour::Concave));
    assert_eq!(record.tail.as_deref(), Some("135"));
    let (defined, _, _, labels) = symbol_frame(&state, false);
    assert!(defined);
    assert_eq!(labels, ["2.5", "2.5", "135"]);
}

#[test]
fn weld_symbol_strip_has_iso_a_identification_line_and_arrow() {
    let (defined, _, shapes, _) = symbol_frame(&WeldPanelState::default(), false);
    assert!(defined);
    assert!(shapes.iter().any(|shape| matches!(shape, egui::Shape::LineSegment { .. })), "ISO A identification dashes");
    let filled = shapes.iter().filter(|shape| matches!(shape, egui::Shape::Path(path) if path.fill != egui::Color32::TRANSPARENT)).count();
    assert!(filled >= 1, "the arrow head is a filled triangle");
}
#[test]
fn weld_panel_type_defaults_and_filtered_process_tables() {
    crate::i18n::install();
    use sections::look::numbers;
    let expected = [
        (WeldType::Fillet, Face::Convex, "135", vec!["111", "121", "131", "135", "136", "138", "141", "311", "51", "52"]),
        (WeldType::Butt, Face::Flat, "135", vec!["111", "121", "131", "135", "136", "138", "141", "311", "51", "52", "42", "43", "185"]),
        (WeldType::Edge, Face::Convex, "141", vec!["141", "131", "135", "311", "15", "52"]),
        (WeldType::Plug, Face::Flat, "135", vec!["111", "131", "135", "141"]),
        (WeldType::Spot, Face::Convex, "21", vec!["21", "52", "51", "41", "135", "141"]),
        (WeldType::Seam, Face::Convex, "22", vec!["22", "52", "51", "41", "43"]),
        (WeldType::Stud, Face::Convex, "78", vec!["78"]),
        (WeldType::Surfacing, Face::Convex, "135", vec!["111", "121", "131", "135", "136", "138", "141", "15", "52", "441"]),
        (WeldType::Tack, Face::Convex, "135", vec!["111", "131", "135", "141"]),
    ];
    let mut state = WeldPanelState::default();
    for (typ, face, process, offered) in expected {
        state.pick_type(typ);
        assert_eq!(state.face, face);
        assert_eq!(state.process, process);
        assert_eq!(numbers(typ), offered);
        assert_ne!(tr(&format!("{}.desc", typ.info().label)), format!("{}.desc", typ.info().label));
    }
    state.tack_mode = 2;
    assert_eq!(state.where_kind(), WhereKind::Point);
    state.tack_mode = 0;
    assert_eq!(state.where_kind(), WhereKind::Line);
    assert!(state.density.is_empty());
    assert!(state.overlap.is_empty());
    assert_eq!(
        TYPES.iter().filter(|info| info.visible).map(|info| info.weld_type).collect::<Vec<_>>(),
        [WeldType::Fillet, WeldType::Butt, WeldType::Spot, WeldType::Tack]
    );
}

#[test]
fn weld_symbol_full_penetration_keeps_contour_side_and_tail() {
    use solidcraft_engine::geom::weld::{Contour, Kind};
    let mut state = WeldPanelState::default();
    state.pick_type(WeldType::Butt);
    state.side = 1;
    let record = symbol::record(&state).unwrap();
    assert!(record.arrow_side.is_none());
    let side = record.other_side.unwrap();
    assert_eq!(side.kind, Kind::SingleVButt);
    assert_eq!(side.size, None);
    assert_eq!(side.contour, Some(Contour::Flush));
    assert_eq!(record.tail.as_deref(), Some("135"));
    state.pick_type(WeldType::Tack);
    assert!(symbol::record(&state).is_err());
}

#[test]
fn weld_symbol_spans_use_length_count_and_centre_pitch_only_when_supported() {
    let mut state = WeldPanelState { spans: true, span_count: "3".into(), span_length: "10".into(), span_pitch: "40".into(), ..Default::default() };
    let record = symbol::record(&state).unwrap();
    let side = record.arrow_side.unwrap();
    assert_eq!(side.length_mm, Some(10.));
    let spans = side.intermittent.unwrap();
    assert_eq!(spans.count, Some(3));
    assert_eq!(spans.pitch_mm, 40.);
    for (count, length, pitch) in [("1", "10", "40"), ("3", "10", "10"), ("3", "bad", "40")] {
        state.span_count = count.into();
        state.span_length = length.into();
        state.span_pitch = pitch.into();
        assert!(symbol::record(&state).unwrap().arrow_side.unwrap().intermittent.is_none());
    }
}

#[test]
fn weld_symbol_settings_change_painted_geometry_or_labels() {
    let baseline = WeldPanelState::default();
    let signature = |state: &WeldPanelState| {
        let (defined, _, shapes, labels) = symbol_frame(state, false);
        assert!(defined);
        (format!("{shapes:?}"), labels)
    };
    let first = signature(&baseline);
    let mut variants = Vec::new();
    let mut state = baseline.clone();
    state.pick_type(WeldType::Butt);
    variants.push(("type", state));
    let mut state = baseline.clone();
    state.size.a = "4".into();
    variants.push(("a", state));
    let mut state = baseline.clone();
    state.size_by_leg = true;
    variants.push(("z", state));
    let mut state = baseline.clone();
    state.face = Face::Concave;
    variants.push(("face", state));
    let mut state = baseline.clone();
    state.side = 1;
    variants.push(("side", state));
    let mut state = baseline.clone();
    state.both_sides = true;
    variants.push(("both", state));
    let mut state = baseline.clone();
    state.spans = true;
    variants.push(("spans", state));
    let mut state = baseline.clone();
    state.process = "141".into();
    variants.push(("process", state));
    let mut state = baseline.clone();
    state.size.s = "5".into();
    variants.push(("deep throat", state));
    let mut state = baseline.clone();
    state.field_weld = true;
    variants.push(("field", state));
    let mut state = baseline.clone();
    state.all_around = true;
    variants.push(("all around", state));
    for (name, state) in variants {
        assert_ne!(first, signature(&state), "{name} must repaint");
    }
    let mut butt = WeldPanelState::default();
    butt.pick_type(WeldType::Butt);
    let full = signature(&butt);
    butt.partial_penetration = true;
    butt.size.s = "2.5".into();
    assert_ne!(full, signature(&butt));
    let partial = signature(&butt);
    butt.size.s = "3".into();
    assert_ne!(partial, signature(&butt));
    butt.groove = 0;
    assert_ne!(partial, signature(&butt));
    let mut leg = baseline.clone();
    leg.size_by_leg = true;
    let first_leg = signature(&leg);
    leg.size.z = "7".into();
    assert_ne!(first_leg, signature(&leg));
}

#[test]
fn weld_standards_use_the_decided_identification_position_and_keep_the_arrow() {
    use solidcraft_engine::geom::weld::{Convention, IdentificationLine};
    let mut state = WeldPanelState::default();
    assert_eq!(symbol::convention(&state), Convention::Iso2553_2019A { identification: IdentificationLine::Above });
    state.side = 1;
    assert_eq!(symbol::convention(&state), Convention::Iso2553_2019A { identification: IdentificationLine::Below });
    for (name, convention) in [("iso2553_2019_b", Convention::Iso2553_2019B), ("aws_a2_4_2020", Convention::AwsA2_4_2020)] {
        state.symbol_standard = name.into();
        state.size_by_leg = true;
        assert_eq!(symbol::convention(&state), convention);
        let (defined, _, shapes, _) = symbol_frame(&state, false);
        assert!(defined);
        assert!(
            shapes.iter().any(|shape| matches!(shape,egui::Shape::Path(path) if path.fill != egui::Color32::TRANSPARENT)),
            "{name} keeps the arrowhead"
        );
    }
}

#[test]
fn weld_equal_leg_hint_uses_the_parsed_number_and_hides_for_expressions() {
    crate::i18n::install();
    let mut state = WeldPanelState::default();
    state.size.a = "5".into();
    assert_eq!(sections::size::leg_hint(&state).unwrap(), "z = 7.071 mm (equal legs)");
    state.size_by_leg = true;
    state.size.z = "10".into();
    assert_eq!(sections::size::leg_hint(&state).unwrap(), "a = 7.071 mm (equal legs)");
    for input in ["plate/2", "3 mm", "NaN", "inf", "", "-1"] {
        state.size.z = input.into();
        assert!(sections::size::leg_hint(&state).is_none(), "{input}");
    }
}

#[test]
fn weld_sketch_dimension_click_focuses_the_corresponding_field() {
    let ctx = context();
    let mut state = WeldPanelState { advanced_open: true, ..WeldPanelState::default() };
    state.size.z1 = "4".into();
    state.size.z2 = "6".into();
    render(&ctx, &mut state, false, vec![]);
    for letter in ["a", "z1", "z2"] {
        let rect = solidcraft_ui_registry::rect_of(&format!("weld:dimension:{letter}")).unwrap();
        click(&ctx, &mut state, rect.center());
        assert!(ctx.memory(|m| m.focused().is_some()), "{letter} focuses an expression editor");
        assert!(state.focus_dimension.is_none(), "{letter} was consumed by its field");
    }
}
