use super::*;
use crate::test_support as harness;
use solidcraft_ui_registry as registry;

fn cell_frame(ctx: &egui::Context, selection: &mut CellSelection, events: Vec<egui::Event>, modifiers: egui::Modifiers) -> egui::FullOutput {
    registry::clear_handles();
    harness::raw_frame(
        ctx,
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 800.0))),
            events: std::iter::once(egui::Event::ModifiersChanged(modifiers)).chain(events).collect(),
            focused: true,
            ..Default::default()
        },
        |ui| {
            ui.set_width(600.0);
            crate::scope(ui, |ui| {
                let columns = ["Name", "Expression", "Value", "Unit"].map(|label| Column::new(label, 1.0).flex());
                table_custom(ui, "cells", &columns, "Empty", |ui, table| {
                    table.cell_selection(selection, SelectionMode::Range);
                    table.headers(ui, |_, _| false);
                    for index in 0..5 {
                        if index == 2 {
                            table.group(ui, "band", "Parameters", 3, &mut true);
                        }
                        table.row(ui, &index.to_string(), false, |row| {
                            for _ in 0..4 {
                                row.cell(|ui| crate::text(ui, "Width", crate::Text::Cell));
                            }
                        });
                    }
                });
            });
        },
    )
}

fn cell_key(ctx: &egui::Context, selection: &mut CellSelection, key: egui::Key, modifiers: egui::Modifiers) {
    cell_frame(ctx, selection, vec![egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }], modifiers);
    cell_frame(ctx, selection, vec![egui::Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers }], modifiers);
}

#[test]
fn cell_selection_coordinates_normalise_ranges_and_clear() {
    let mut selection = CellSelection::default();
    assert!(selection.is_empty());
    assert_eq!(selection.single(), None);
    selection.anchor = Some((3, 4));
    selection.focus = Some((1, 2));
    assert_eq!(selection.range(), Some((1..=3, 2..=4)));
    assert!(selection.contains(2, 3));
    assert!(!selection.contains(0, 3));
    selection.focus = selection.anchor;
    assert_eq!(selection.single(), Some((3, 4)));
    selection.clear();
    assert!(selection.is_empty());
}

#[test]
fn cell_range_is_one_snapped_fill_and_inside_outline_across_hairlines_and_bands() {
    for dark in [false, true] {
        let t = Tokens::themed(dark, solidcraft_theme::ColorVision::default());
        let _tokens = solidcraft_theme::scoped_tokens(t);
        for ppp in [1.0, 1.25, 2.0] {
            let ctx = harness::context();
            ctx.set_pixels_per_point(ppp);
            ctx.enable_accesskit();
            let mut selection = CellSelection { anchor: Some((1, 1)), focus: Some((3, 3)) };
            let output = cell_frame(&ctx, &mut selection, vec![], Default::default());
            let first = registry::rect_of("dialog:table:cells:1:cell:Expression").unwrap();
            let last = registry::rect_of("dialog:table:cells:3:cell:Unit").unwrap();
            let expected = first.union(last).expand(t.space_4);
            let block = registry::rect_of("dialog:table:cells:selection").unwrap();
            assert!((block.min - expected.min).abs().max_elem() <= 1.0 / ppp);
            assert!((block.max - expected.max).abs().max_elem() <= 1.0 / ppp);
            assert_eq!(registry::count_of("dialog:table:cells:selection:cells"), Some(9.0));
            let tree = output.platform_output.accesskit_update.as_ref().unwrap();
            let selected_cells = tree.nodes.iter().filter(|(_, node)| node.role() == egui::Role::Cell && node.is_selected() == Some(true)).count();
            assert_eq!(selected_cells, 9);
            let background = t.panel.blend(t.table_fill()).blend(t.form_primary.gamma_multiply(0.15));
            assert!(solidcraft_theme::contrast_ratio(t.form_ink, background) >= 4.5);
            let mut fills = Vec::new();
            let mut outlines = Vec::new();
            for (index, s) in output.shapes.iter().enumerate() {
                harness::each_shape(&s.shape, &mut |shape| {
                    if let egui::Shape::Rect(r) = shape {
                        if r.fill == t.form_primary.gamma_multiply(0.15) {
                            fills.push((index, r.clone()));
                        }
                        if r.stroke == egui::Stroke::new(1.5, t.form_primary) {
                            outlines.push((index, r.clone()));
                        }
                    }
                    if let egui::Shape::LineSegment { points, .. } = shape {
                        let crosses = points[0].y > block.top()
                            && points[0].y < block.bottom()
                            && points[0].y == points[1].y
                            && points[0].x < block.right()
                            && points[1].x > block.left();
                        assert!(!crosses, "an internal hairline cuts the selection: {points:?}");
                    }
                });
            }
            assert_eq!(fills.len(), 1);
            assert_eq!(outlines.len(), 1);
            assert_eq!(fills[0].1.rect, block);
            assert_eq!(outlines[0].1.rect, block);
            assert_eq!(outlines[0].1.corner_radius, egui::CornerRadius::same(t.control_radius));
            assert_eq!(outlines[0].1.stroke_kind, egui::StrokeKind::Inside);
            let text = output.shapes.iter().position(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.job.text == "Width")).unwrap();
            assert!(fills[0].0 < text && outlines[0].0 < text, "contents remain above the block");
        }
    }
}

#[test]
fn cells_shift_click_drag_keyboard_escape_and_select_all() {
    let ctx = harness::context();
    let mut selection = CellSelection::default();
    cell_frame(&ctx, &mut selection, vec![], Default::default());
    let first = registry::rect_of("dialog:table:cells:1:cell:Expression").unwrap().center();
    let last = registry::rect_of("dialog:table:cells:3:cell:Unit").unwrap().center();
    for pressed in [true, false] {
        cell_frame(&ctx, &mut selection, harness::pointer(first, pressed), Default::default());
    }
    assert_eq!(selection.single(), Some((1, 1)));
    for pressed in [true, false] {
        cell_frame(&ctx, &mut selection, harness::pointer(last, pressed), egui::Modifiers::SHIFT);
    }
    assert_eq!(selection.range(), Some((1..=3, 1..=3)));
    cell_key(&ctx, &mut selection, egui::Key::ArrowUp, Default::default());
    assert_eq!(selection.single(), Some((2, 3)), "bands do not count as rows");
    cell_key(&ctx, &mut selection, egui::Key::ArrowLeft, egui::Modifiers::SHIFT);
    assert_eq!(selection.range(), Some((2..=2, 2..=3)));
    cell_key(&ctx, &mut selection, egui::Key::Home, Default::default());
    assert_eq!(selection.single(), Some((2, 0)));
    cell_key(&ctx, &mut selection, egui::Key::End, Default::default());
    assert_eq!(selection.single(), Some((2, 3)));
    cell_key(&ctx, &mut selection, egui::Key::A, egui::Modifiers::CTRL);
    assert_eq!(selection.range(), Some((0..=4, 0..=3)));
    cell_key(&ctx, &mut selection, egui::Key::Escape, Default::default());
    assert!(selection.is_empty());
    cell_frame(&ctx, &mut selection, harness::pointer(first, true), Default::default());
    cell_frame(&ctx, &mut selection, vec![egui::Event::PointerMoved(last)], Default::default());
    cell_frame(&ctx, &mut selection, harness::pointer(last, false), Default::default());
    assert_eq!(selection.range(), Some((1..=3, 1..=3)));
    cell_frame(&ctx, &mut selection, harness::pointer(first, true), Default::default());
    cell_frame(&ctx, &mut selection, vec![egui::Event::PointerMoved(last)], Default::default());
    cell_key(&ctx, &mut selection, egui::Key::Escape, Default::default());
    assert!(selection.is_empty(), "Esc cancels the drag as well as its current range");
    cell_frame(&ctx, &mut selection, harness::pointer(last, false), Default::default());
    assert!(selection.is_empty(), "releasing a cancelled drag does not recreate the range");
}

#[test]
fn hostile_cell_indices_are_clamped_after_shrinking_or_emptying_the_table() {
    let ctx = harness::context();
    let mut selection = CellSelection { anchor: Some((usize::MAX, usize::MAX)), focus: Some((usize::MAX - 1, usize::MAX)) };
    cell_frame(&ctx, &mut selection, vec![], Default::default());
    assert_eq!(selection.single(), Some((4, 3)));
    painted_frame(&ctx, vec![], |ui| {
        table_custom(ui, "empty_cells", &[], "Empty", |_, table| table.cell_selection(&mut selection, SelectionMode::Range));
    });
    assert!(selection.is_empty());
}

#[test]
fn single_cell_mode_paints_one_field_ring_and_does_not_extend() {
    let ctx = harness::context();
    let t = Tokens::get();
    let mut selection = CellSelection { anchor: Some((0, 0)), focus: Some((1, 1)) };
    let output = painted_frame(&ctx, vec![], |ui| {
        let columns = [Column::new("Name", 1.0).flex(), Column::new("Value", 1.0).flex()];
        table_custom(ui, "single_cell", &columns, "Empty", |ui, table| {
            table.cell_selection(&mut selection, SelectionMode::Cell);
            for index in 0..2 {
                table.row(ui, &index.to_string(), false, |row| {
                    row.cell(|ui| crate::text(ui, "Width", crate::Text::Cell));
                    row.cell(|ui| crate::text(ui, "40 mm", crate::Text::MonoCell));
                });
            }
        });
    });
    assert_eq!(selection.single(), Some((1, 1)));
    let block = registry::rect_of("dialog:table:single_cell:selection").unwrap();
    let cell = registry::rect_of("dialog:table:single_cell:1:cell:Value").unwrap();
    assert_eq!(block, cell.expand(t.space_4), "the ring has padding without moving the text");
    assert_eq!(output.shapes.iter().filter(|s| matches!(&s.shape, egui::Shape::Rect(r) if r.rect == block && r.fill == t.input_fill())).count(), 1);
    assert_eq!(
        output
            .shapes
            .iter()
            .filter(|s| matches!(&s.shape, egui::Shape::Rect(r) if r.rect == block
        && r.stroke == egui::Stroke::new(1.5, t.form_primary) && r.corner_radius == egui::CornerRadius::same(t.control_radius)
        && r.stroke_kind == egui::StrokeKind::Inside))
            .count(),
        1
    );
}

#[test]
fn cell_range_records_virtual_rows_and_frozen_scrolled_cells_without_rebuilding_content() {
    let ctx = harness::context();
    let mut selection = CellSelection { anchor: Some((0, 0)), focus: Some((7, 2)) };
    let mut built = Vec::new();
    let mut expected = egui::Rect::NOTHING;
    let scroll = std::cell::Cell::new(0.0);
    {
        let mut draw = |ui: &mut Ui| {
            built.clear();
            expected = egui::Rect::NOTHING;
            ui.set_clip_rect(egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(600.0, 100.0)));
            let columns = [
                Column::new("Name", 1.0).frozen(),
                Column::new("Expression", 1.0).min_content_width(300.0),
                Column::new("Value", 1.0).min_content_width(300.0),
            ];
            table_custom(ui, "virtual_cells", &columns, "Empty", |ui, table| {
                table.cell_selection(&mut selection, SelectionMode::Range);
                scroll.set(table.scroll_x);
                table.headers(ui, |_, _| false);
                for index in 0..8 {
                    table.row_visible(ui, &index.to_string(), false, false, |row| {
                        built.push(index);
                        for _ in 0..3 {
                            row.cell(|ui| crate::text(ui, "Width", crate::Text::Cell));
                        }
                    });
                }
                assert_eq!(table.cells.len(), 24, "all rows have exact recorded geometry");
                for cell in &table.cells {
                    expected = expected.union(cell.rect);
                }
                let frozen = table.cells.first().unwrap().rect;
                let scrolling = table.cells.get(1).unwrap().rect;
                assert!(scrolling.left() >= frozen.right(), "scrolling cells stay behind the frozen region");
            });
        };
        painted_frame(&ctx, vec![], &mut draw);
        let pointer = registry::rect_of("dialog:table:virtual_cells:0:cell:Expression").unwrap().center();
        painted_frame(
            &ctx,
            vec![
                egui::Event::PointerMoved(pointer),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: vec2(-80.0, 0.0),
                    modifiers: Default::default(),
                    phase: egui::TouchPhase::Move,
                    source: egui::MouseWheelSource::Unknown,
                },
            ],
            &mut draw,
        );
        painted_frame(&ctx, vec![], &mut draw);
    }
    assert!(scroll.get() > 0.0, "the horizontal viewport actually scrolled");
    let block = registry::rect_of("dialog:table:virtual_cells:selection").unwrap();
    let expected = expected.expand(Tokens::get().space_4);
    assert!((block.min - expected.min).abs().max_elem() <= 1.0);
    assert!((block.max - expected.max).abs().max_elem() <= 1.0);
    assert!(built.contains(&0));
    assert!(!built.contains(&7));
}

#[test]
fn cell_selection_leaves_buttons_and_editors_in_charge_of_their_clicks() {
    let ctx = harness::context();
    let mut selection = CellSelection::default();
    let mut value = "10 mm".to_owned();
    let clicked = std::cell::Cell::new(false);
    let editor = std::cell::Cell::new(egui::Id::NULL);
    let button = std::cell::Cell::new(egui::Rect::NOTHING);
    let field = std::cell::Cell::new(egui::Rect::NOTHING);
    let dropdown = std::cell::Cell::new(egui::Rect::NOTHING);
    let dropdown_clicked = std::cell::Cell::new(false);
    let copied = std::cell::Cell::new(false);
    let copy_rect = std::cell::Cell::new(egui::Rect::NOTHING);
    {
        let mut draw = |ui: &mut Ui| {
            let columns = ["Name", "Value", "Unit", "Expression"].map(|label| Column::new(label, 1.0).flex());
            table_custom(ui, "interactive_cells", &columns, "Empty", |ui, table| {
                table.cell_selection(&mut selection, SelectionMode::Range);
                table.row(ui, "row", false, |row| {
                    row.cell(|ui| {
                        let response = crate::button(ui, "Delete", crate::Button::Small);
                        button.set(response.rect);
                        clicked.set(clicked.get() || response.clicked());
                    });
                    row.cell(|ui| {
                        let response = crate::cell_editor(ui, ui.scope_id().with("edit"), &mut value, "", false);
                        editor.set(response.id);
                        field.set(response.rect);
                    });
                    row.cell(|ui| {
                        let response = crate::dropdown(ui, "unit", &mut 0, &["mm", "in"]);
                        dropdown.set(response.rect);
                        dropdown_clicked.set(dropdown_clicked.get() || response.clicked());
                    });
                    row.cell(|ui| {
                        copy_rect.set(ui.max_rect());
                        let activated = crate::copy_cell(ui, "40 mm", "Width", true, "");
                        copied.set(copied.get() || activated);
                    });
                });
            });
        };
        painted_frame(&ctx, vec![], &mut draw);
        for at in [button.get().center(), field.get().center()] {
            for pressed in [true, false] {
                painted_frame(&ctx, harness::pointer(at, pressed), &mut draw);
            }
        }
        assert!(clicked.get());
        assert!(ctx.memory(|memory| memory.has_focus(editor.get())));
        painted_frame(
            &ctx,
            vec![
                egui::Event::ModifiersChanged(egui::Modifiers { ctrl: true, command: true, ..Default::default() }),
                egui::Event::Key {
                    key: egui::Key::A,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers { ctrl: true, command: true, ..Default::default() },
                },
            ],
            &mut draw,
        );
        painted_frame(&ctx, vec![egui::Event::ModifiersChanged(egui::Modifiers::NONE), egui::Event::Text("25 mm".into())], &mut draw);
        for at in [copy_rect.get().center(), dropdown.get().center()] {
            for pressed in [true, false] {
                painted_frame(&ctx, harness::pointer(at, pressed), &mut draw);
            }
        }
    }
    assert_eq!(value, "25 mm", "Ctrl+A belongs to the focused editor");
    assert!(copied.get());
    assert!(dropdown_clicked.get());
    assert!(selection.is_empty());
}

#[test]
fn fitting_flex_tables_have_no_horizontal_scrollbar_at_card_widths_and_pixel_scales() {
    for ppp in [1.0, 1.25, 2.0] {
        for width in [260.0, 319.0, 320.0, 499.0, 500.3, 748.0, 749.0, 762.0] {
            let ctx = crate::test_support::context();
            ctx.set_pixels_per_point(ppp);
            for _ in 0..3 {
                harness::frame(&ctx, vec![], |ui| {
                    ui.set_width(width);
                    // Include a non-zero inherited scroll margin: the grid owns its insets.
                    ui.spacing_mut().scroll.content_margin = egui::Margin::same(1);
                    table_custom(
                        ui,
                        "fit",
                        &[Column::new("Name", 1.1).flex(), Column::new("Expression", 1.5).flex(), Column::new("Value", 1.1).flex()],
                        "Empty",
                        |ui, table| {
                            table.headers(ui, |_, _| false);
                            table.row(ui, "row", false, |row| {
                                row.cell(|ui| {
                                    crate::text(ui, "Width", crate::Text::Cell);
                                });
                                row.cell(|ui| {
                                    crate::cell_editor(ui, ui.scope_id().with("value"), &mut "10 mm".into(), "", false);
                                });
                                row.cell(|ui| {
                                    crate::text(ui, "10 mm", crate::Text::MonoCell);
                                });
                            });
                            table.add_prompt(ui, "Add parameter", "Add parameter");
                        },
                    );
                });
            }
            let viewport = registry::rect_of("dialog:table:fit:viewport").unwrap();
            let content = registry::count_of("dialog:table:fit:content_width").unwrap() as f32;
            assert!(content <= viewport.width().ceil(), "{width} at {ppp}: content {content}, viewport {viewport:?}");
            let row = registry::rect_of("dialog:table:fit:row").unwrap();
            let first = registry::rect_of("dialog:table:fit:row:cell:Name").unwrap();
            let last = registry::rect_of("dialog:table:fit:row:cell:Value").unwrap();
            assert_eq!(first.left() - row.left(), Tokens::get().space_8);
            assert!(
                (row.right() - last.right() - Tokens::get().space_8).abs() <= egui::emath::GUI_ROUNDING,
                "width {width} at {ppp}: row {row:?} last {last:?}"
            );
            for label in ["Name", "Expression", "Value"] {
                let header = registry::rect_of(&format!("dialog:column:fit:{label}")).unwrap();
                let cell = registry::rect_of(&format!("dialog:table:fit:row:cell:{label}")).unwrap();
                assert_eq!(header.x_range(), cell.x_range());
            }
        }
    }
}

#[test]
fn genuinely_wide_tables_keep_horizontal_scrolling() {
    let ctx = crate::test_support::context();
    harness::frame(&ctx, vec![], |ui| {
        table_custom(ui, "overflow", &[Column::new("Name", 1.0).flex().min_content_width(400.0)], "Empty", |ui, table| {
            table.headers(ui, |_, _| false);
            table.row(ui, "row", false, |row| {
                row.cell(|ui| crate::text(ui, "Name", crate::Text::Cell));
            });
        });
    });
    assert!(
        registry::count_of("dialog:table:overflow:content_width").unwrap()
            > f64::from(registry::rect_of("dialog:table:overflow:viewport").unwrap().width().ceil())
    );
}

#[test]
fn row_hover_paints_under_every_interactive_child_and_respects_clip_and_enabled() {
    let ctx = crate::test_support::context();
    let mut value = "10 mm".to_owned();
    let mut selected = 0;
    let mut draw = |ui: &mut Ui| {
        ui.set_width(600.0);
        table_custom(
            ui,
            "hover",
            &[
                Column::new("Text", 1.0).flex(),
                Column::new("Field", 1.0).flex(),
                Column::new("Choice", 1.0).flex(),
                Column::new("Action", 1.0).flex(),
            ],
            "Empty",
            |ui, table| {
                table.headers(ui, |_, _| false);
                table.row(ui, "row", false, |row| {
                    row.cell(|ui| {
                        crate::text(ui, "Width", crate::Text::Cell);
                    });
                    row.cell(|ui| {
                        crate::cell_editor(ui, ui.scope_id().with("value"), &mut value, "", false);
                    });
                    row.cell(|ui| {
                        crate::dropdown(ui, "choice", &mut selected, &["mm", "in"]);
                    });
                    row.cell(|ui| {
                        crate::button(ui, "Delete", crate::Button::Normal);
                    });
                });
            },
        );
    };
    let mut output = ctx
        .run_ui(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 400.0))), ..Default::default() }, |ui| {
            crate::scope(ui, &mut draw)
        });
    output.textures_delta.clear();
    let row = registry::rect_of("dialog:table:hover:row").unwrap();
    let mut targets = vec![row.left_center() + vec2(1.0, 0.0)];
    for handle in ["dialog:cell:Width", "dialog:Field", "dialog:Choice", "dialog:button:Action:Delete"] {
        targets.push(registry::handle_at(handle).unwrap());
    }
    for (enabled, clipped) in [(true, false), (false, false), (true, true)] {
        for at in &targets {
            for _ in 0..2 {
                let mut output = ctx.run_ui(egui::RawInput { events: vec![egui::Event::PointerMoved(*at)], ..Default::default() }, |ui| {
                    if clipped {
                        ui.set_clip_rect(egui::Rect::from_min_max(egui::Pos2::ZERO, row.left_top()));
                    }
                    ui.add_enabled_ui(enabled, |ui| crate::scope(ui, &mut draw));
                });
                output.textures_delta.clear();
                let highlight = output
                    .shapes
                    .iter()
                    .any(|shape| matches!(&shape.shape, egui::Shape::Rect(paint) if paint.rect == row && paint.fill == Tokens::get().text.gamma_multiply(0.06)));
                assert_eq!(highlight, enabled && !clipped, "pointer {at:?}, enabled {enabled}, clipped {clipped}");
            }
        }
    }
}

#[test]
fn add_prompt_is_short_rounded_and_uses_a_hand_cursor() {
    let ctx = crate::test_support::context();
    let mut draw = |ui: &mut Ui| {
        table_custom(ui, "prompt", &[Column::new("Name", 1.0).flex()], "Empty", |ui, table| {
            table.headers(ui, |_, _| false);
            table.add_prompt(ui, "Add configuration", "Add configuration");
        });
    };
    harness::frame(&ctx, vec![], &mut draw);
    let rect = registry::rect_of("dialog:table:prompt:add").unwrap();
    assert_eq!(rect.height(), row_height() - Tokens::get().space_4);
    assert_eq!(harness::frame_cursor(&ctx, rect.center(), &mut draw), egui::CursorIcon::PointingHand);
    let idle = crate::test_support::context();
    let mut output = idle.run_ui(egui::RawInput::default(), |ui| {
        ui.set_width(280.0);
        crate::scope(ui, draw);
    });
    output.textures_delta.clear();
    // A solid, visible border in the normal control colour, not a faint dash.
    let rect = registry::rect_of("dialog:table:prompt:add").unwrap();
    let solid = output.shapes.iter().flat_map(|s| match &s.shape {
        egui::Shape::Vec(shapes) => shapes.as_slice(),
        shape => std::slice::from_ref(shape),
    });
    let solid = solid
        .filter(|s| matches!(s, egui::Shape::Rect(r) if r.rect == rect && r.stroke.color == Tokens::get().control_border && r.stroke.width > 0.0))
        .count();
    assert_eq!(solid, 1);
}

#[test]
fn hover_remove_bin_prompt_and_plus_menu_share_one_grid() {
    for dark in [false, true] {
        let ctx = crate::test_support::context();
        ctx.set_visuals(if dark { egui::Visuals::dark() } else { egui::Visuals::light() });
        let mut pending = false;
        let mut removed = false;
        let mut deleted = false;
        let mut menu_open = false;
        let mut draw = |ui: &mut Ui| {
            add_column_button(ui, |ui| {
                menu_open = true;
                crate::text(ui, "Column choices", crate::Text::Inline);
            });
            table_custom(
                ui,
                "shared",
                &[Column::new("Name", 1.0), Column::new("Actions", 0.0).icon_only().min_content_width(40.0)],
                "Empty",
                |ui, table| {
                    table.headers(ui, |ui, index| {
                        if index == 0 {
                            removed |= removable_header(ui, "Name");
                        }
                        true
                    });
                    if pending {
                        table.row(ui, "draft", false, |row| {
                            row.cell(|ui| {
                                crate::cell_editor(ui, ui.scope_id().with("name"), &mut String::new(), "Name", false);
                            });
                            row.cell(|ui| {
                                deleted |= bin_button(ui, "Delete").clicked();
                            });
                        });
                    }
                    if table.add_prompt(ui, "Add parameter", "Add parameter").clicked() {
                        pending = true;
                    }
                },
            );
        };
        for _ in 0..3 {
            harness::frame(&ctx, vec![], &mut draw);
        }
        assert!(registry::handle_at("dialog:button:Name:×").is_none());
        let header = registry::rect_of("dialog:column:shared:Name").unwrap();
        harness::frame(&ctx, vec![egui::Event::PointerMoved(header.center())], &mut draw);
        let remove = registry::handle_at("dialog:button:Name:×").unwrap();
        assert!(header.contains(remove));
        harness::frame(&ctx, harness::pointer(remove, true), &mut draw);
        harness::frame(&ctx, harness::pointer(remove, false), &mut draw);
        let add = registry::handle_at("dialog:table:shared:add").unwrap();
        harness::frame(&ctx, harness::pointer(add, true), &mut draw);
        harness::frame(&ctx, harness::pointer(add, false), &mut draw);
        harness::frame(&ctx, vec![], &mut draw);
        let row = registry::rect_of("dialog:table:shared:draft").unwrap();
        assert_eq!(row.height(), crate::table_row_height());
        assert!(registry::rect_of("dialog:column:shared:Name").unwrap().bottom() <= row.top());
        assert!(row.bottom() <= registry::rect_of("dialog:table:shared:add").unwrap().top());
        let bin = registry::handle_at("dialog:button:Actions:Delete").unwrap();
        assert!(row.contains(bin));
        harness::frame(&ctx, harness::pointer(bin, true), &mut draw);
        harness::frame(&ctx, harness::pointer(bin, false), &mut draw);
        let plus = registry::handle_at("dialog:menu:Add Column").unwrap();
        harness::frame(&ctx, harness::pointer(plus, true), &mut draw);
        harness::frame(&ctx, harness::pointer(plus, false), &mut draw);
        harness::frame(&ctx, vec![], &mut draw);
        assert!(pending && removed && deleted && menu_open);
    }
}

#[test]
fn add_column_is_a_labelled_button_of_normal_height() {
    let ctx = crate::test_support::context();
    let draw = |ui: &mut Ui| {
        add_column_button(ui, |_| {});
    };
    let render = |events| {
        let mut output = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
            crate::scope(ui, draw);
        });
        output.textures_delta.clear();
        output
    };
    let out = render(vec![]);
    let rect = registry::rect_of("dialog:menu:Add Column").unwrap();
    assert_eq!(rect.height(), Tokens::get().control_height);
    assert!(rect.width() > rect.height() * 2.0, "the button carries its label");
    let fills = |out: &egui::FullOutput, fill| {
        out.shapes.iter().filter(|s| matches!(&s.shape, egui::Shape::Rect(r) if r.rect == rect && r.fill == fill)).count()
    };
    assert_eq!(fills(&out, Tokens::get().control_fill), 1, "idle add-column is filled like a normal button");
    render(vec![egui::Event::PointerMoved(rect.center())]);
    let out = render(vec![egui::Event::PointerMoved(rect.center())]);
    assert_eq!(fills(&out, Tokens::get().control_fill), 0, "hover highlights it");
}

#[test]
fn horizontal_bar_is_painted_only_for_actual_table_overflow() {
    for overflow in [false, true] {
        let ctx = crate::test_support::context();
        ctx.global_style_mut(|style| style.spacing.scroll = egui::style::ScrollStyle::solid());
        for frame in 0..4 {
            let mut output = ctx.run_ui(
                egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(320.0, 400.0))), ..Default::default() },
                |ui| {
                    ui.set_width(280.0);
                    crate::scope(ui, |ui| {
                        let column = Column::new("Name", 1.0).flex().min_content_width(if overflow { 400.0 } else { 0.0 });
                        table_custom(ui, "bar", &[column], "Empty", |ui, table| {
                            table.headers(ui, |_, _| false);
                            table.row(ui, "row", false, |row| {
                                row.cell(|ui| crate::text(ui, "Width", crate::Text::Cell));
                            });
                        });
                    });
                },
            );
            output.textures_delta.clear();
            let bottom = registry::rect_of("dialog:table:bar:row").unwrap().bottom();
            let bar = output.shapes.iter().any(|shape| {
                matches!(&shape.shape,
                egui::Shape::Rect(r) if r.fill.a() > 0 && r.rect.width() > 100.0 && r.rect.height() <= 10.0
                    && r.rect.top() >= bottom && r.rect.bottom() <= bottom + 20.0)
            });
            if frame == 3 {
                assert_eq!(bar, overflow);
            }
        }
    }
}

/// #8097: a stored file's long name takes the width the size and actions leave (no empty band
/// on the right), and is only cut, with the full name on hover, when even that is too narrow.
#[test]
fn name_list_gives_the_name_the_width_the_other_cells_leave() {
    let long = "a-rather-long-stored-design-name-for-the-bracket-v12.solidcraft";
    for (width, cut) in [(900.0, false), (420.0, true)] {
        let ctx = crate::test_support::context();
        for _ in 0..4 {
            harness::frame(&ctx, vec![], |ui| {
                ui.set_width(width);
                crate::name_list(ui, "files", &["Name", "Size", "Actions"], "Empty", |list| {
                    for name in [long, "b.step"] {
                        list.row(name, |row| {
                            row.name(name);
                            row.cell(|ui| ui.label("12.3 KB"));
                            row.cell(|ui| {
                                crate::button(ui, "Open", crate::Button::Small);
                                crate::button(ui, "Save to computer…", crate::Button::Small);
                                crate::button(ui, "Delete", crate::Button::Small);
                            });
                        });
                    }
                });
            });
        }
        let t = Tokens::get();
        let row = registry::rect_of(&format!("dialog:table:files:{long}")).unwrap();
        let name = registry::rect_of(&format!("dialog:table:files:{long}:cell:Name")).unwrap();
        let size = registry::rect_of(&format!("dialog:table:files:{long}:cell:Size")).unwrap();
        let actions = registry::rect_of(&format!("dialog:table:files:{long}:cell:Actions")).unwrap();
        // The actions end at the row's inset: nothing is left over on the right.
        assert!((row.right() - actions.right() - t.space_8).abs() <= 1.0, "{width}: row {row:?} actions {actions:?}");
        // The size and actions are as wide as their content, not wider; the name has the rest.
        assert!(size.width() < 80.0 && actions.width() < 320.0, "{width}: size {size:?} actions {actions:?}");
        // ...and never narrower: the size is not cut and every action button keeps its label.
        let ctx_label = |text: &str| ctx.fonts_mut(|f| f.layout_no_wrap(text.into(), egui::FontId::proportional(t.font_label), t.form_ink).size().x);
        assert!(size.width() >= ctx_label("12.3 KB") * 0.8, "{width}: size {size:?}");
        assert!(actions.width() >= ctx_label("OpenSave to computer…Delete"), "{width}: actions {actions:?}");
        assert!((size.left() - name.right() - t.space_8).abs() <= 1.0);
        assert_eq!(registry::count_of(&format!("dialog:name:{long}:cut")), Some(f64::from(u8::from(cut))), "{width}: name {name:?}");
        assert_eq!(registry::count_of("dialog:name:b.step:cut"), Some(0.0));
    }
}

/// #8563: names sort in natural order: numbers by value, letters without regard to case.
#[test]
fn natural_order_compares_numbers_by_value_and_ignores_case() {
    let mut names = vec!["d10", "width", "d2", "Alpha", "d1", "alpha", "Beta", "d02", "x", "x1", "p99999999999999999999999", "p100"];
    Sort::Ascending.apply(&mut names, |s| *s);
    assert_eq!(names, ["Alpha", "alpha", "Beta", "d1", "d2", "d02", "d10", "p100", "p99999999999999999999999", "width", "x", "x1"]);
    Sort::Descending.apply(&mut names, |s| *s);
    assert_eq!(names, ["x1", "x", "width", "p99999999999999999999999", "p100", "d10", "d02", "d2", "d1", "Beta", "alpha", "Alpha"]);
    let mut listed = vec!["d10", "d2", "d1"];
    Sort::Listed.apply(&mut listed, |s| *s);
    assert_eq!(listed, ["d10", "d2", "d1"], "the listed order is left alone");
    assert_eq!(natural_cmp("", ""), std::cmp::Ordering::Equal);
    assert_eq!(natural_cmp("", "a"), std::cmp::Ordering::Less);
    assert_eq!([Sort::Listed.next(), Sort::Ascending.next(), Sort::Descending.next()], [Sort::Ascending, Sort::Descending, Sort::Listed]);
}

/// A frozen first column stays at the table's left edge while the others scroll sideways beneath
/// it, its heading with it, and the scrolled cells are clipped where they pass under it (T-693).
#[test]
fn a_frozen_column_stays_in_view_while_the_table_scrolls_sideways() {
    let ctx = crate::test_support::context();
    let columns = [
        Column::new("Name", 1.0).flex().min_content_width(120.0).frozen(),
        Column::new("Wide A", 1.0).flex().min_content_width(400.0),
        Column::new("Wide B", 1.0).flex().min_content_width(400.0),
    ];
    let mut clips = Vec::new();
    // With `scroll`, the last cell asks to be shown, which scrolls the table to its right end.
    let draw = |scroll: bool, clips: &mut Vec<egui::Rect>| {
        clips.clear();
        harness::frame(&ctx, vec![], |ui| {
            ui.set_width(360.0);
            ui.style_mut().scroll_animation = egui::style::ScrollAnimation::none();
            table_custom(ui, "frozen", &columns, "Empty", |ui, table| {
                table.headers(ui, |_, _| false);
                table.row(ui, "row", false, |row| {
                    row.cell(|ui| crate::text(ui, "Default", crate::Text::Cell));
                    row.cell(|ui| clips.push(ui.clip_rect()));
                    row.cell(|ui| {
                        clips.push(ui.clip_rect());
                        if scroll {
                            ui.scroll_to_rect(ui.max_rect(), Some(egui::Align::Max));
                        }
                    });
                });
            });
        });
    };
    draw(false, &mut clips);
    draw(false, &mut clips);
    let name = registry::rect_of("dialog:table:frozen:row:cell:Name").unwrap();
    let wide = registry::rect_of("dialog:table:frozen:row:cell:Wide A").unwrap();
    draw(true, &mut clips);
    draw(false, &mut clips);
    draw(false, &mut clips);
    let scrolled = registry::rect_of("dialog:table:frozen:row:cell:Wide A").unwrap();
    assert!(scrolled.left() < wide.left() - 100.0, "the table scrolled sideways: {wide:?} -> {scrolled:?}");
    assert_eq!(registry::rect_of("dialog:table:frozen:row:cell:Name").unwrap().x_range(), name.x_range(), "the frozen cell stays put");
    assert_eq!(registry::rect_of("dialog:column:frozen:Name").unwrap().x_range(), name.x_range(), "its heading too");
    for clip in &clips {
        assert!(clip.left() >= name.right(), "a scrolled cell is clipped beside the frozen one: {clip:?} vs {name:?}");
    }
}

/// A frozen column wider than half the visible table stops being frozen, so the other columns
/// can still be scrolled into view.
#[test]
fn a_frozen_column_too_wide_for_the_view_scrolls_with_the_rest() {
    let ctx = crate::test_support::context();
    let columns = [Column::new("Name", 1.0).flex().min_content_width(300.0).frozen(), Column::new("Wide A", 1.0).flex().min_content_width(400.0)];
    let draw = |scroll: bool| {
        harness::frame(&ctx, vec![], |ui| {
            ui.set_width(360.0);
            ui.style_mut().scroll_animation = egui::style::ScrollAnimation::none();
            table_custom(ui, "too_wide", &columns, "Empty", |ui, table| {
                table.headers(ui, |_, _| false);
                table.row(ui, "row", false, |row| {
                    row.cell(|ui| crate::text(ui, "Default", crate::Text::Cell));
                    row.cell(|ui| {
                        if scroll {
                            ui.scroll_to_rect(ui.max_rect(), Some(egui::Align::Max));
                        }
                    });
                });
            });
        });
    };
    draw(false);
    draw(false);
    let name = registry::rect_of("dialog:table:too_wide:row:cell:Name").unwrap();
    draw(true);
    draw(false);
    draw(false);
    assert!(registry::rect_of("dialog:table:too_wide:row:cell:Name").unwrap().left() < name.left() - 100.0, "the name column scrolled away");
}

fn painted_frame(ctx: &egui::Context, events: Vec<egui::Event>, mut add: impl FnMut(&mut Ui)) -> egui::FullOutput {
    registry::clear_handles();
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 800.0))),
            events,
            focused: true,
            ..Default::default()
        },
        |ui| {
            ui.set_width(600.0);
            crate::scope(ui, &mut add);
        },
    );
    output.textures_delta.clear();
    output
}

#[test]
fn card_and_header_are_translucent_in_both_themes_and_keep_the_token_geometry() {
    let previous = solidcraft_theme::is_dark();
    for dark in [false, true] {
        solidcraft_theme::set_dark(dark);
        let t = Tokens::get();
        let ctx = crate::test_support::context();
        let output = painted_frame(&ctx, vec![], |ui| {
            table_custom(ui, "card", &[Column::new("Name", 1.0).flex()], "Empty", |ui, table| {
                table.headers(ui, |_, _| false);
                table.row(ui, "row", false, |_| {});
                table.add_prompt(ui, "Add parameter", "Add parameter");
            });
        });
        let card = output
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Rect(r) if r.fill == t.table_fill() => Some(r),
                _ => None,
            })
            .unwrap();
        assert!(card.fill.a() > 0 && card.fill.a() < 255);
        assert_eq!(card.corner_radius, egui::CornerRadius::same(t.radius_lg));
        assert_eq!(card.stroke, t.container_border(t.border));
        assert_eq!(card.rect.left(), t.space_12);
        assert_eq!(card.rect.top(), t.space_12);
        let row = registry::rect_of("dialog:table:card:row").unwrap();
        let viewport = registry::rect_of("dialog:table:card:viewport").unwrap();
        assert_eq!(row.left() - viewport.left(), t.space_4);
        assert_eq!(viewport.right() - row.right(), t.space_4);
        let header = registry::rect_of("dialog:column:card:Name").unwrap();
        assert_eq!(header.height(), row_height() + t.space_6);
        assert_eq!(row.height(), crate::table_row_height());
        assert!(output.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Rect(r) if r.fill == t.text.gamma_multiply(0.08)
            && r.rect.width() == viewport.width() && r.rect.height() == header.height())));
    }
    solidcraft_theme::set_dark(previous);
}

#[test]
fn consecutive_selection_is_one_rounded_painted_block_without_internal_rules_or_alpha_overlap() {
    for ppp in [1.0, 1.25, 2.0] {
        let ctx = crate::test_support::context();
        ctx.set_pixels_per_point(ppp);
        let output = painted_frame(&ctx, vec![], |ui| {
            table_custom(ui, "selection", &[Column::new("Name", 1.0).flex()], "Empty", |ui, table| {
                table.headers(ui, |_, _| false);
                for (key, selected) in [("before", false), ("a", true), ("b", true), ("c", true), ("after", false)] {
                    table.row_visible(ui, key, selected, false, |row| {
                        row.cell(|ui| crate::text(ui, key, crate::Text::Cell));
                    });
                }
            });
        });
        let t = Tokens::get();
        let rows: Vec<_> = ["a", "b", "c"].map(|key| registry::rect_of(&format!("dialog:table:selection:{key}")).unwrap()).into();
        for pair in rows.windows(2) {
            assert_eq!(pair[0].bottom(), pair[1].top(), "no layout gap at {ppp}");
            assert!(
                !output.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::LineSegment { points, .. }
                if points[0].y == pair[0].bottom() && points[1].y == pair[0].bottom())),
                "no internal hairline at {ppp}"
            );
        }
        let union = rows[0].union(rows[2]);
        let fills: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Rect(r) if r.fill == t.selected_bg => Some(r),
                _ => None,
            })
            .collect();
        assert_eq!(fills.len(), 1, "one fill means no overlapping alpha or rounded internal seams");
        assert_eq!(fills[0].rect, union);
        assert_eq!(fills[0].corner_radius, egui::CornerRadius::same(t.radius_md));
        let bars: Vec<_> = output.shapes.iter().filter(|s| matches!(&s.shape, egui::Shape::Rect(r) if r.fill == t.form_primary)).collect();
        assert_eq!(bars.len(), 1);
        assert_eq!(bars[0].clip_rect, egui::Rect::from_min_max(union.min, egui::pos2(union.left() + t.space_3, union.bottom())));
        let last = registry::rect_of("dialog:table:selection:after").unwrap();
        assert!(
            !output.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::LineSegment { points, .. }
            if points[0].y == last.bottom() && points[1].y == last.bottom())),
            "no last-row hairline"
        );
    }
}

#[test]
fn section_counts_are_filled_round_pills_in_both_themes() {
    for dark in [false, true] {
        let t = Tokens::themed(dark, solidcraft_theme::ColorVision::default());
        let _tokens = solidcraft_theme::scoped_tokens(t);
        let ctx = harness::context();
        let output = painted_frame(&ctx, vec![], |ui| {
            table_custom(ui, "pill", &[Column::new("Name", 1.0).flex()], "Empty", |ui, table| {
                table.group(ui, "group", "Parameters", 12, &mut false);
            });
        });
        let pills: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Rect(r) if r.rect.height() == t.space_16 && r.fill == t.text.gamma_multiply(0.13) => Some(r),
                _ => None,
            })
            .collect();
        assert_eq!(pills.len(), 1);
        assert_eq!(pills[0].stroke, egui::Stroke::NONE);
        assert_eq!(pills[0].corner_radius, egui::CornerRadius::same((t.space_16 / 2.0) as u8));
        assert!(output.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Text(text)
            if text.galley.job.text == "12" && text.galley.job.sections[0].format.color == t.form_dim)));
    }
}

#[test]
fn selected_hover_strengthens_the_same_block_and_focus_is_inside_and_rounded() {
    let t = Tokens::get();
    let ctx = harness::context();
    let ids = std::cell::Cell::new([egui::Id::NULL; 2]);
    let draw = |ui: &mut Ui| {
        table_custom(ui, "states", &[Column::new("Name", 1.0).flex()], "Empty", |ui, table| {
            let group = table.group(ui, "group", "Parameters", 1, &mut true);
            let row = table.row(ui, "selected", true, |_| {});
            ids.set([group.id, row.id]);
        });
    };
    let idle = painted_frame(&ctx, vec![], draw);
    let rect = registry::rect_of("dialog:table:states:selected").unwrap();
    assert!(idle.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Rect(r) if r.rect == rect && r.fill == t.selected_bg)));
    let hovered = painted_frame(&ctx, vec![egui::Event::PointerMoved(rect.center())], draw);
    assert!(hovered.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Rect(r)
        if r.rect == rect && r.fill == t.selected_bg.lerp_to_gamma(t.form_primary, 0.08))));
    assert!(!hovered.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Rect(r) if r.fill == t.text.gamma_multiply(0.06))));
    for (index, bounds) in [registry::rect_of("dialog:table:states:group:group").unwrap(), rect].into_iter().enumerate() {
        ctx.memory_mut(|m| m.request_focus(ids.get()[index]));
        let output = painted_frame(&ctx, vec![], draw);
        let rings: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Rect(r) if r.stroke == egui::Stroke::new(1.5, t.form_primary) => Some(r),
                _ => None,
            })
            .collect();
        assert_eq!(rings.len(), 1);
        assert!(bounds.contains_rect(rings[0].rect));
        assert_eq!(rings[0].stroke_kind, egui::StrokeKind::Inside);
        assert_eq!(rings[0].corner_radius, egui::CornerRadius::same(t.radius_md));
    }
}

#[test]
fn first_and_last_selected_rows_stay_within_the_rounded_card() {
    let t = Tokens::get();
    let ctx = harness::context();
    let output = painted_frame(&ctx, vec![], |ui| {
        table_custom(ui, "edges", &[Column::new("Name", 1.0).flex()], "Empty", |ui, table| {
            table.row(ui, "first", true, |_| {});
            table.row(ui, "last", true, |_| {});
        });
    });
    let card = output
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Rect(r) if r.fill == t.table_fill() && r.stroke.width > 0.0 => Some(r.rect),
            _ => None,
        })
        .unwrap();
    let selected = output
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Rect(r) if r.fill == t.selected_bg => Some(r),
            _ => None,
        })
        .unwrap();
    assert!(selected.rect.left() >= card.left() + t.space_4);
    assert!(selected.rect.right() <= card.right() - t.space_4);
    // Sample the rounded selection's four arcs against the card's corner circles.
    for sx in [-1.0, 1.0] {
        for sy in [-1.0, 1.0] {
            let corner = egui::pos2(if sx < 0.0 { card.left() } else { card.right() }, if sy < 0.0 { card.top() } else { card.bottom() });
            let center = corner - vec2(sx, sy) * f32::from(t.radius_lg);
            let r = f32::from(t.radius_md);
            let selection_center = egui::pos2(
                if sx < 0.0 { selected.rect.left() + r } else { selected.rect.right() - r },
                if sy < 0.0 { selected.rect.top() + r } else { selected.rect.bottom() - r },
            );
            for step in 0..=32 {
                let angle = step as f32 / 32.0 * std::f32::consts::FRAC_PI_2;
                let p = selection_center + vec2(sx * angle.cos(), sy * angle.sin()) * r;
                assert!(card.contains(p));
                if (p.x - center.x) * sx > 0.0 && (p.y - center.y) * sy > 0.0 {
                    assert!(p.distance(center) <= f32::from(t.radius_lg) + 0.01, "selection pokes past card: {p:?}");
                }
            }
        }
    }
}

#[test]
fn focused_table_dropdown_paints_one_inside_control_radius_ring() {
    let ctx = harness::context();
    let t = Tokens::get();
    let id = std::cell::Cell::new(egui::Id::NULL);
    let rect = std::cell::Cell::new(egui::Rect::NOTHING);
    let mut draw = |ui: &mut Ui| {
        table_custom(ui, "choice_ring", &[Column::new("Unit", 1.0).flex()], "Empty", |ui, table| {
            table.row(ui, "row", false, |row| {
                row.cell(|ui| {
                    let response = crate::dropdown(ui, "unit", &mut 0, &["mm", "in"]);
                    id.set(response.id);
                    rect.set(response.rect);
                });
            });
        });
    };
    painted_frame(&ctx, vec![], &mut draw);
    ctx.memory_mut(|m| m.request_focus(id.get()));
    let output = painted_frame(&ctx, vec![], &mut draw);
    let mut rings = Vec::new();
    for s in &output.shapes {
        harness::each_shape(&s.shape, &mut |shape| {
            if let egui::Shape::Rect(r) = shape
                && r.stroke.width > 0.0
                && r.rect.width() <= rect.get().width() + 4.0
                && r.rect.intersects(rect.get())
            {
                assert!(s.clip_rect.contains_rect(r.rect));
                rings.push(r.clone());
            }
        });
    }
    assert_eq!(rings.len(), 1, "{rings:?}");
    assert_eq!(rings[0].stroke, egui::Stroke::new(1.5, crate::selection_ink(&t)));
    assert_eq!(rings[0].stroke_kind, egui::StrokeKind::Inside);
    assert_eq!(rings[0].corner_radius, egui::CornerRadius::same(t.control_radius));
}

#[test]
fn active_configuration_pill_keeps_an_opaque_contrasting_surface_on_selection() {
    for dark in [false, true] {
        let t = Tokens::themed(dark, solidcraft_theme::ColorVision::default());
        let _tokens = solidcraft_theme::scoped_tokens(t);
        let ctx = harness::context();
        let output = painted_frame(&ctx, vec![], |ui| {
            table_custom(ui, "active", &[Column::new("Name", 1.0).flex()], "Empty", |ui, table| {
                table.row(ui, "configuration", true, |row| {
                    row.cell(active_pill);
                });
            });
        });
        let rect = registry::rect_of("dialog:inline:Active").unwrap();
        assert!(output.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Rect(r) if r.rect == rect && r.fill == t.control_fill)));
        assert_eq!(t.control_fill.a(), 255);
        assert!(solidcraft_theme::contrast_ratio(t.form_primary, t.control_fill) >= 4.5);
        assert!(
            harness::text_shapes(&output).any(|text| text.galley.job.text == "Active" && text.galley.job.sections[0].format.color == t.form_primary)
        );
    }
}

#[test]
fn section_bands_toggle_by_pointer_space_and_enter_with_stable_identity() {
    let ctx = crate::test_support::context();
    let open = std::cell::Cell::new(false);
    let id = std::cell::Cell::new(egui::Id::NULL);
    let mut draw = |ui: &mut Ui| {
        table_custom(ui, "groups", &[Column::new("Name", 1.0).flex()], "Empty", |ui, table| {
            table.headers(ui, |_, _| false);
            let mut value = open.get();
            let response = table.group(ui, "first", "Parameters", 2, &mut value);
            id.set(response.id);
            open.set(value);
            if value {
                table.row(ui, "row", false, |_| {});
            }
        });
    };
    painted_frame(&ctx, vec![], &mut draw);
    let band = registry::rect_of("dialog:table:groups:group:first").unwrap();
    assert_eq!(band.height(), row_height() - Tokens::get().space_2);
    // Far from the chevron: the entire band is the target.
    let at = band.right_center() - vec2(Tokens::get().space_4, 0.0);
    painted_frame(&ctx, harness::pointer(at, true), &mut draw);
    painted_frame(&ctx, harness::pointer(at, false), &mut draw);
    assert!(open.get());
    let stable_id = id.get();
    for (key, expected) in [(egui::Key::Space, false), (egui::Key::Enter, true)] {
        ctx.memory_mut(|memory| memory.request_focus(stable_id));
        painted_frame(&ctx, harness::key(key), &mut draw);
        assert_eq!(open.get(), expected);
        assert_eq!(id.get(), stable_id);
        painted_frame(
            &ctx,
            vec![egui::Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers: Default::default() }],
            &mut draw,
        );
    }
    assert!(registry::rect_of("dialog:table:groups:row").is_some());
}

#[test]
fn empty_groups_do_not_hide_the_empty_state_and_nonempty_collapsed_groups_do() {
    for count in [0, 2] {
        let ctx = crate::test_support::context();
        let output = painted_frame(&ctx, vec![], |ui| {
            table_custom(ui, "empty_groups", &[Column::new("Name", 1.0).flex()], "Empty", |ui, table| {
                table.headers(ui, |_, _| false);
                table.group(ui, "group", "Parameters", count, &mut false);
                table.add_prompt(ui, "Add parameter", "Add parameter");
            });
        });
        let empty = output
            .shapes
            .iter()
            .filter(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.job.text == solidcraft_ui_text::tr_label("Empty")))
            .count();
        assert_eq!(empty, usize::from(count == 0));
        assert!(registry::rect_of("dialog:table:empty_groups:add").is_some());
    }
}

#[test]
fn section_bands_break_selection_blocks_and_keep_header_and_cell_columns_aligned() {
    let ctx = crate::test_support::context();
    let output = painted_frame(&ctx, vec![], |ui| {
        table_custom(ui, "sections", &[Column::new("Name", 1.0).flex()], "Empty", |ui, table| {
            table.headers(ui, |_, _| false);
            for key in ["first", "second"] {
                table.group(ui, key, "Parameters", 1, &mut true);
                table.row(ui, key, true, |row| {
                    row.cell(|ui| crate::text(ui, "Width", crate::Text::Cell));
                });
            }
            table.add_prompt(ui, "Add parameter", "Add parameter");
        });
    });
    let t = Tokens::get();
    let fills = output.shapes.iter().filter(|s| matches!(&s.shape, egui::Shape::Rect(r) if r.fill == t.selected_bg)).count();
    assert_eq!(fills, 2);
    for key in ["first", "second"] {
        let band = registry::rect_of(&format!("dialog:table:sections:group:{key}")).unwrap();
        let row = registry::rect_of(&format!("dialog:table:sections:{key}")).unwrap();
        let cell = registry::rect_of(&format!("dialog:table:sections:{key}:cell:Name")).unwrap();
        assert_eq!(band.bottom(), row.top());
        assert_eq!(band.left() + t.space_4, row.left());
        assert_eq!(cell.x_range(), registry::rect_of("dialog:column:sections:Name").unwrap().x_range());
        assert!(output.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::LineSegment { points, stroke }
            if points[0].y == band.bottom() && points[1].y == band.bottom() && *stroke == t.tertiary_border(t.dialog_rule))));
    }
}

#[test]
fn virtual_rows_keep_selection_geometry_and_forced_cells_and_reveal_targets() {
    let ctx = crate::test_support::context();
    let mut built = Vec::new();
    let mut rects = Vec::new();
    let output = painted_frame(&ctx, vec![], |ui| {
        ui.set_clip_rect(egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(600.0, 100.0)));
        table_custom(ui, "virtual", &[Column::new("Name", 1.0).flex()], "Empty", |ui, table| {
            table.headers(ui, |_, _| false);
            for index in 0..8 {
                let response = table.row_visible(ui, &index.to_string(), true, index == 7, |_| built.push(index));
                rects.push(response.rect);
                if index == 7 {
                    table.reveal_row(response.rect);
                    assert_eq!(table.reveal, Some(response.rect));
                }
            }
        });
    });
    assert!(built.contains(&0) && built.contains(&7));
    assert!(!built.contains(&6));
    for pair in rects.windows(2) {
        assert_eq!(pair[0].bottom(), pair[1].top());
    }
    let union = rects[0].union(rects[7]);
    let fills: Vec<_> = output
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Rect(r) if r.fill == Tokens::get().selected_bg => Some((r.rect, s.clip_rect)),
            _ => None,
        })
        .collect();
    assert_eq!(fills.len(), 1);
    assert_eq!(fills[0].0, union);
    assert_eq!(fills[0].1.bottom(), 100.0, "deferred paint keeps the original clip");
}

#[test]
fn hostile_column_counts_weights_and_minimums_are_bounded() {
    let ctx = crate::test_support::context();
    painted_frame(&ctx, vec![], |ui| {
        let columns: Vec<_> = [f32::NAN, f32::INFINITY, -1.0, f32::MAX]
            .into_iter()
            .cycle()
            .take(1500)
            .map(|width| Column::new("Name", width).flex().min_content_width(width))
            .collect();
        table_custom(ui, "bounded", &columns, "Empty", |ui, table| {
            table.headers(ui, |_, _| false);
            assert_eq!(table.columns.len(), MAX_COLUMNS);
            // Upstream grids support more than the old card implementation's 64 columns.
            assert!(table.columns.len() > 64);
            assert!(table.widths.iter().all(|width| width.is_finite() && *width >= 0.0 && *width <= Tokens::get().form_card_width * 64.0));
            table.row(ui, "row", false, |_| {});
        });
    });
    assert!(registry::rect_of("dialog:table:bounded:row").unwrap().is_finite());
}
