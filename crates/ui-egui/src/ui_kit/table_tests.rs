use super::*;
use crate::ui_kit::{self, test_support as harness};

#[test]
fn fitting_flex_tables_have_no_horizontal_scrollbar_at_card_widths_and_pixel_scales() {
    for ppp in [1.0, 1.25, 2.0] {
        for width in [260.0, 319.0, 320.0, 499.0, 500.3, 748.0, 749.0, 762.0] {
            let ctx = crate::fonts::test_context();
            ctx.set_pixels_per_point(ppp);
            for _ in 0..3 {
                harness::frame(&ctx, vec![], |ui| {
                    ui.set_width(width);
                    // Include a non-zero inherited scroll margin: the grid owns its insets.
                    ui.spacing_mut().scroll.content_margin = egui::Margin::same(1);
                    table(
                        ui,
                        "fit",
                        &[Column::new("Name", 1.1).flex(), Column::new("Expression", 1.5).flex(), Column::new("Value", 1.1).flex()],
                        "Empty",
                        |ui, table| {
                            table.row(ui, "row", false, |row| {
                                row.cell(|ui| {
                                    ui_kit::text(ui, "Width", ui_kit::Text::Cell);
                                });
                                row.cell(|ui| {
                                    ui_kit::cell_editor(ui, ui.id().with("value"), &mut "10 mm".into(), "", false);
                                });
                                row.cell(|ui| {
                                    ui_kit::text(ui, "10 mm", ui_kit::Text::MonoCell);
                                });
                            });
                            table.add_prompt(ui, "Add parameter", "Add parameter");
                        },
                    );
                });
            }
            let viewport = scenario::rect_of("dialog:table:fit:viewport").unwrap();
            let content = scenario::count_of("dialog:table:fit:content_width").unwrap() as f32;
            assert!(content <= viewport.width().ceil(), "{width} at {ppp}: content {content}, viewport {viewport:?}");
            let row = scenario::rect_of("dialog:table:fit:row").unwrap();
            let first = scenario::rect_of("dialog:table:fit:row:cell:Name").unwrap();
            let last = scenario::rect_of("dialog:table:fit:row:cell:Value").unwrap();
            assert_eq!(first.left() - row.left(), Tokens::get().space_8);
            assert!(
                (row.right() - last.right() - Tokens::get().space_8).abs() <= egui::emath::GUI_ROUNDING,
                "width {width} at {ppp}: row {row:?} last {last:?}"
            );
            for label in ["Name", "Expression", "Value"] {
                let header = scenario::rect_of(&format!("dialog:column:fit:{label}")).unwrap();
                let cell = scenario::rect_of(&format!("dialog:table:fit:row:cell:{label}")).unwrap();
                assert_eq!(header.x_range(), cell.x_range());
            }
        }
    }
}

#[test]
fn genuinely_wide_tables_keep_horizontal_scrolling() {
    let ctx = crate::fonts::test_context();
    harness::frame(&ctx, vec![], |ui| {
        table(ui, "overflow", &[Column::new("Name", 1.0).flex().min_content_width(400.0)], "Empty", |ui, table| {
            table.row(ui, "row", false, |row| {
                row.cell(|ui| ui_kit::text(ui, "Name", ui_kit::Text::Cell));
            });
        });
    });
    assert!(
        scenario::count_of("dialog:table:overflow:content_width").unwrap()
            > f64::from(scenario::rect_of("dialog:table:overflow:viewport").unwrap().width().ceil())
    );
}

#[test]
fn row_hover_paints_under_every_interactive_child_and_respects_clip_and_enabled() {
    let ctx = crate::fonts::test_context();
    let mut value = "10 mm".to_owned();
    let mut selected = 0;
    let mut draw = |ui: &mut Ui| {
        ui.set_width(600.0);
        table(
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
                table.row(ui, "row", false, |row| {
                    row.cell(|ui| {
                        ui_kit::text(ui, "Width", ui_kit::Text::Cell);
                    });
                    row.cell(|ui| {
                        ui_kit::cell_editor(ui, ui.id().with("value"), &mut value, "", false);
                    });
                    row.cell(|ui| {
                        ui_kit::dropdown(ui, "choice", &mut selected, &["mm", "in"]);
                    });
                    row.cell(|ui| {
                        ui_kit::button(ui, "Delete", ui_kit::Button::Normal);
                    });
                });
            },
        );
    };
    let mut output = ctx
        .run_ui(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 400.0))), ..Default::default() }, |ui| {
            ui_kit::scope(ui, &mut draw)
        });
    output.textures_delta.clear();
    let row = scenario::rect_of("dialog:table:hover:row").unwrap();
    let mut targets = vec![row.left_center() + vec2(1.0, 0.0)];
    for handle in ["dialog:cell:Width", "dialog:Field", "dialog:Choice", "dialog:button:Action:Delete"] {
        targets.push(scenario::handle_at(handle).unwrap());
    }
    for (enabled, clipped) in [(true, false), (false, false), (true, true)] {
        for at in &targets {
            for _ in 0..2 {
                let mut output = ctx.run_ui(egui::RawInput { events: vec![egui::Event::PointerMoved(*at)], ..Default::default() }, |ui| {
                    if clipped {
                        ui.set_clip_rect(egui::Rect::from_min_max(egui::Pos2::ZERO, row.left_top()));
                    }
                    ui.add_enabled_ui(enabled, |ui| ui_kit::scope(ui, &mut draw));
                });
                output.textures_delta.clear();
                let highlight = output
                    .shapes
                    .iter()
                    .any(|shape| matches!(&shape.shape, egui::Shape::Rect(paint) if paint.rect == row && paint.fill == Tokens::get().hover));
                assert_eq!(highlight, enabled && !clipped, "pointer {at:?}, enabled {enabled}, clipped {clipped}");
            }
        }
    }
}

#[test]
fn add_prompt_is_short_rounded_and_uses_a_hand_cursor() {
    let ctx = crate::fonts::test_context();
    let mut draw = |ui: &mut Ui| {
        table(ui, "prompt", &[Column::new("Name", 1.0).flex()], "Empty", |ui, table| {
            table.add_prompt(ui, "Add configuration", "Add configuration");
        });
    };
    harness::frame(&ctx, vec![], &mut draw);
    let rect = scenario::rect_of("dialog:table:prompt:add").unwrap();
    assert_eq!(rect.height(), row_height() - Tokens::get().space_4);
    assert_eq!(harness::frame_cursor(&ctx, rect.center(), &mut draw), egui::CursorIcon::PointingHand);
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        ui.set_width(280.0);
        ui_kit::scope(ui, draw);
    });
    output.textures_delta.clear();
    // The dashed outline follows a rounded path and leaves each rectangular corner empty.
    let shapes = output.shapes.iter().flat_map(|s| match &s.shape {
        egui::Shape::Vec(shapes) => shapes.as_slice(),
        shape => std::slice::from_ref(shape),
    });
    let dashed = shapes
        .filter_map(|shape| match shape {
            egui::Shape::LineSegment { points, stroke } if stroke.color == Tokens::get().control_border => Some(points),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(!dashed.is_empty());
    for corner in [rect.left_top(), rect.right_top(), rect.left_bottom(), rect.right_bottom()] {
        assert!(dashed.iter().flat_map(|points| points.iter()).all(|p| p.distance(corner) > f32::from(Tokens::get().control_radius) * 0.2));
    }
}

#[test]
fn hover_remove_bin_prompt_and_plus_menu_share_one_grid() {
    for dark in [false, true] {
        let ctx = crate::fonts::test_context();
        ctx.set_visuals(if dark { egui::Visuals::dark() } else { egui::Visuals::light() });
        let mut pending = false;
        let mut removed = false;
        let mut deleted = false;
        let mut menu_open = false;
        let mut draw = |ui: &mut Ui| {
            table_custom(
                ui,
                "shared",
                &[Column::new("Name", 1.0), Column::new("Actions", 0.0).icon_only().min_content_width(40.0)],
                "Empty",
                |ui, table| {
                    table.headers(ui, |ui, index| {
                        if index == 0 {
                            removed |= removable_header(ui, "Name");
                        } else {
                            plus_header(ui, |ui| {
                                menu_open = true;
                                ui_kit::text(ui, "Column choices", ui_kit::Text::Inline);
                            });
                        }
                        true
                    });
                    if pending {
                        table.row(ui, "draft", false, |row| {
                            row.cell(|ui| {
                                ui_kit::cell_editor(ui, ui.id().with("name"), &mut String::new(), "Name", false);
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
        assert!(scenario::handle_at("dialog:button:Name:×").is_none());
        let header = scenario::rect_of("dialog:column:shared:Name").unwrap();
        harness::frame(&ctx, vec![egui::Event::PointerMoved(header.center())], &mut draw);
        let remove = scenario::handle_at("dialog:button:Name:×").unwrap();
        assert!(header.contains(remove));
        harness::frame(&ctx, harness::pointer(remove, true), &mut draw);
        harness::frame(&ctx, harness::pointer(remove, false), &mut draw);
        let add = scenario::handle_at("dialog:table:shared:add").unwrap();
        harness::frame(&ctx, harness::pointer(add, true), &mut draw);
        harness::frame(&ctx, harness::pointer(add, false), &mut draw);
        harness::frame(&ctx, vec![], &mut draw);
        let row = scenario::rect_of("dialog:table:shared:draft").unwrap();
        assert_eq!(row.height(), crate::ui_kit::table_row_height());
        assert!(scenario::rect_of("dialog:column:shared:Name").unwrap().bottom() <= row.top());
        assert!(row.bottom() <= scenario::rect_of("dialog:table:shared:add").unwrap().top());
        let bin = scenario::handle_at("dialog:button:Actions:Delete").unwrap();
        assert!(row.contains(bin));
        harness::frame(&ctx, harness::pointer(bin, true), &mut draw);
        harness::frame(&ctx, harness::pointer(bin, false), &mut draw);
        let plus = scenario::handle_at("dialog:menu:Add Column").unwrap();
        harness::frame(&ctx, harness::pointer(plus, true), &mut draw);
        harness::frame(&ctx, harness::pointer(plus, false), &mut draw);
        harness::frame(&ctx, vec![], &mut draw);
        assert!(pending && removed && deleted && menu_open);
    }
}

#[test]
fn add_column_is_small_and_unfilled_until_hovered() {
    let ctx = crate::fonts::test_context();
    let draw = |ui: &mut Ui| {
        ui.allocate_ui(egui::vec2(40.0, 36.0), |ui| {
            ui.set_min_size(egui::vec2(40.0, 36.0));
            plus_header(ui, |_| {});
        });
    };
    let render = |events| {
        let mut output = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
            ui_kit::scope(ui, draw);
        });
        output.textures_delta.clear();
        output
    };
    let out = render(vec![]);
    let rect = scenario::rect_of("dialog:menu:Add Column").unwrap();
    assert!((20.0..=22.0).contains(&rect.width()));
    assert_eq!(rect.width(), rect.height());
    let fills =
        |out: &egui::FullOutput| out.shapes.iter().filter(|s| matches!(&s.shape, egui::Shape::Rect(r) if r.rect == rect && r.fill.a() != 0)).count();
    assert_eq!(fills(&out), 0, "idle add-column has no background");
    let out = render(vec![egui::Event::PointerMoved(rect.center())]);
    assert!(out.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Rect(r) if r.rect == rect && r.fill == Tokens::get().selected_bg)));
    let guides = out.shapes.iter().filter(|s| matches!(&s.shape, egui::Shape::LineSegment { points, stroke } if points[0].x == points[1].x && stroke.color == Tokens::get().form_primary)).count();
    assert_eq!(guides, 2, "hover retains both insertion guides");
}

#[test]
fn horizontal_bar_is_painted_only_for_actual_table_overflow() {
    for overflow in [false, true] {
        let ctx = crate::fonts::test_context();
        ctx.global_style_mut(|style| style.spacing.scroll = egui::style::ScrollStyle::solid());
        for frame in 0..4 {
            let mut output = ctx.run_ui(
                egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(320.0, 400.0))), ..Default::default() },
                |ui| {
                    ui.set_width(280.0);
                    ui_kit::scope(ui, |ui| {
                        let column = Column::new("Name", 1.0).flex().min_content_width(if overflow { 400.0 } else { 0.0 });
                        table(ui, "bar", &[column], "Empty", |ui, table| {
                            table.row(ui, "row", false, |row| {
                                row.cell(|ui| ui_kit::text(ui, "Width", ui_kit::Text::Cell));
                            });
                        });
                    });
                },
            );
            output.textures_delta.clear();
            let bottom = scenario::rect_of("dialog:table:bar:row").unwrap().bottom();
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
        let ctx = crate::fonts::test_context();
        for _ in 0..4 {
            harness::frame(&ctx, vec![], |ui| {
                ui.set_width(width);
                ui_kit::name_list(ui, "files", &["Name", "Size", "Actions"], "Empty", |list| {
                    for name in [long, "b.step"] {
                        list.row(name, |row| {
                            row.name(name);
                            row.cell(|ui| ui.label("12.3 KB"));
                            row.cell(|ui| {
                                ui_kit::button(ui, "Open", ui_kit::Button::Small);
                                ui_kit::button(ui, "Save to computer…", ui_kit::Button::Small);
                                ui_kit::button(ui, "Delete", ui_kit::Button::Small);
                            });
                        });
                    }
                });
            });
        }
        let t = Tokens::get();
        let row = scenario::rect_of(&format!("dialog:table:files:{long}")).unwrap();
        let name = scenario::rect_of(&format!("dialog:table:files:{long}:cell:Name")).unwrap();
        let size = scenario::rect_of(&format!("dialog:table:files:{long}:cell:Size")).unwrap();
        let actions = scenario::rect_of(&format!("dialog:table:files:{long}:cell:Actions")).unwrap();
        // The actions end at the row's inset: nothing is left over on the right.
        assert!((row.right() - actions.right() - t.space_8).abs() <= 1.0, "{width}: row {row:?} actions {actions:?}");
        // The size and actions are as wide as their content, not wider; the name has the rest.
        assert!(size.width() < 80.0 && actions.width() < 320.0, "{width}: size {size:?} actions {actions:?}");
        // ...and never narrower: the size is not cut and every action button keeps its label.
        let ctx_label = |text: &str| ctx.fonts_mut(|f| f.layout_no_wrap(text.into(), egui::FontId::proportional(t.font_label), t.form_ink).size().x);
        assert!(size.width() >= ctx_label("12.3 KB") * 0.8, "{width}: size {size:?}");
        assert!(actions.width() >= ctx_label("OpenSave to computer…Delete"), "{width}: actions {actions:?}");
        assert!((size.left() - name.right() - t.space_8).abs() <= 1.0);
        assert_eq!(scenario::count_of(&format!("dialog:name:{long}:cut")), Some(f64::from(u8::from(cut))), "{width}: name {name:?}");
        assert_eq!(scenario::count_of("dialog:name:b.step:cut"), Some(0.0));
    }
}
