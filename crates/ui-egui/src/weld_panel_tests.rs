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
    let board = [
        ("header", 44.0),
        ("strip", 60.0),
        // The revised board uses a 66-point choice row (8 points taller than its first export).
        ("type", 154.0),
        ("where", 170.0),
        ("size", 162.0),
        ("face", 133.0),
        ("extent", 184.0),
        ("flags", 80.0),
        ("look", 161.0),
        ("notices", 150.0),
        ("buttons", 48.0),
        ("advanced", 35.0),
    ];
    println!(
        "Weld fillet measured heights: {:?}; total {}",
        m.sections.iter().map(|(key, rect)| (*key, rect.height())).collect::<Vec<_>>(),
        m.card.expect("card").height()
    );
    assert_eq!(m.sections.iter().map(|(key, _)| *key).collect::<Vec<_>>(), board.iter().map(|(key, _)| *key).collect::<Vec<_>>());
    close(m.card.expect("card").width(), 344.0, 0.5);
    for (key, height) in board {
        close(section(&m, key).height(), height, 8.0);
    }
    for pair in m.sections.windows(2) {
        close(pair[0].1.bottom(), pair[1].1.top(), 0.5);
    }
    close(m.card.expect("card").height(), 1382.0, 12.0);
}

#[test]
fn weld_panel_size_columns_align_centre_and_have_seven_point_gaps() {
    for typ in [WeldType::Fillet, WeldType::Groove, WeldType::Spot, WeldType::Plug, WeldType::Stud, WeldType::Overlay] {
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
    for typ in [WeldType::Fillet, WeldType::Groove, WeldType::Spot, WeldType::Plug, WeldType::Stud, WeldType::Overlay, WeldType::FrictionStir] {
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
    let symbol = m.widgets.first().expect("symbol").center();
    assert_eq!(click(&ctx, &mut state, symbol), WeldPanelEvent::DefineSymbol);
    let cancel = m.widgets.get(1).expect("cancel").center();
    assert_eq!(click(&ctx, &mut state, cancel), WeldPanelEvent::Cancel);
    let done = m.widgets.get(2).expect("done").center();
    assert_eq!(click(&ctx, &mut state, done), WeldPanelEvent::Done);
    let groove = m.widgets.get(4).expect("groove choice").center();
    assert_eq!(click(&ctx, &mut state, groove), WeldPanelEvent::TypePicked(WeldType::Groove));
    assert_eq!(state.weld_type, WeldType::Groove);
    assert_eq!(state.size.a, "3.0");
    assert_eq!(state.picks.line.len(), 2);
}

#[test]
fn weld_panel_type_table_covers_the_board_and_has_registered_icons() {
    crate::i18n::install();
    assert_eq!(TYPES.len(), 26);
    assert_eq!(TYPES.iter().filter(|info| info.visible).count(), 4);
    assert_eq!(TYPES.iter().filter(|info| info.look_tbd).count(), 5);
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
    assert_eq!(click(&ctx, &mut state, section(&m, "buttons").center()), WeldPanelEvent::NewBead);
    click(&ctx, &mut state, section(&m, "advanced").center());
    assert!(state.advanced_open);
    let (_, advanced) = render(&ctx, &mut state, false, vec![]);
    assert_eq!(advanced.fields.len(), 3);
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
            assert_eq!(text.iter().map(|(label, _)| label.as_str()).collect::<Vec<_>>(), [tr("ui.weld.section.advanced"), tr("ui.weld.pro_fields")]);
            close(row.height(), 36.0, 0.5);
            println!("dark={dark}, open={open}: card {card:?}, painted {bounds:?}, Advanced {row:?}, text {text:?}");
        }
    }
}

#[test]
fn weld_panel_whole_advanced_row_toggles_from_either_state() {
    for open in [false, true] {
        // Include the leading padding, chevron, title, hint and trailing padding.
        for offset in [2.0, 20.0, 60.0, 180.0, 340.0] {
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
            for pair in card.sections.windows(2) {
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

#[test]
fn weld_panel_symbol_strip_fits_preview_hint_and_right_aligned_actions() {
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
        let strip = section(&m, "strip");
        let preview = m.widgets.first().expect("symbol preview");
        let hint = m.strip_hint.expect("hint column");
        let hint_text = m.strip_hint_text.expect("hint text");
        let cancel = m.widgets[1];
        let done = m.widgets[2];
        assert!((109.5..=150.5).contains(&preview.width()));
        let remaining = strip.width() - 24.0 - cancel.width() - done.width() - 96.0 - 3.0 * 6.0;
        close(preview.width(), remaining.clamp(110.0, 150.0), 0.5);
        close(preview.height(), 44.0, 0.5);
        close(hint.width(), 96.0, 0.5);
        assert_eq!(m.strip_hint_lines, 2);
        close(hint_text.center().y, strip.center().y, 0.5);
        close(strip.right() - done.right(), 12.0, 0.5);
        close(preview.left() - strip.left(), 12.0, 0.5);
        let items = [*preview, hint, cancel, done];
        for item in items {
            assert!(m.card.expect("card").expand(0.5).contains_rect(item));
            close(item.center().y, strip.center().y, 0.5);
        }
        for pair in items.windows(2) {
            assert!(!pair[0].intersects(pair[1]), "strip items overlap: {pair:?}");
            close(pair[1].left() - pair[0].right(), 6.0, 0.5);
        }
        assert!(hint.expand(0.5).contains_rect(hint_text));
        let painted_hint = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.job.text == tr("ui.weld.symbol") => Some(text),
                _ => None,
            })
            .expect("the external hint is painted");
        assert!(!painted_hint.galley.elided);
        assert_eq!(painted_hint.galley.rows.len(), 2);
        assert!(painted_hint.galley.job.sections.iter().all(|section| section.format.font_id.size == 11.5 && section.format.color == t.form_dim));
        assert_eq!(painted_hint.galley.rect.translate(painted_hint.pos.to_vec2()), hint_text);
        let icon = m.symbol_icon.expect("symbol icon");
        close(icon.center().x, preview.center().x, 0.5);
        close(icon.center().y, preview.center().y, 0.5);
        let check = m.done_check.expect("Done check");
        close(check.width(), 14.0, 0.5);
        close(check.height(), 14.0, 0.5);
        assert!(m.widgets[2].contains_rect(check));
        assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Rect(r) if r.rect == strip && r.fill == t.dialog_footer)));
        assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::LineSegment { points, stroke } if stroke.color == t.dialog_rule && points.iter().all(|p| (p.y - strip.bottom()).abs() < 0.5))));
        assert!(!output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == tr("ui.weld.symbol") && preview.intersects(text.galley.rect.translate(text.pos.to_vec2())))));
        println!(
            "dark={dark}: strip-relative item rects (x,y,width,height): {:?}; hint text {:?}, {} lines",
            items.map(|r| (r.left() - strip.left(), r.top() - strip.top(), r.width(), r.height())),
            hint_text.translate(-strip.min.to_vec2()),
            m.strip_hint_lines
        );
    }
}
