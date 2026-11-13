use super::*;
use crate::test_support as harness;
use solidcraft_ui_registry as registry;

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
                    .any(|shape| matches!(&shape.shape, egui::Shape::Rect(paint) if paint.rect == row && paint.fill == Tokens::get().hover));
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
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        ui.set_width(280.0);
        crate::scope(ui, draw);
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
        let ctx = crate::test_support::context();
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
                                crate::text(ui, "Column choices", crate::Text::Inline);
                            });
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
fn add_column_is_small_and_unfilled_until_hovered() {
    let ctx = crate::test_support::context();
    let draw = |ui: &mut Ui| {
        ui.allocate_ui(egui::vec2(40.0, 36.0), |ui| {
            ui.set_min_size(egui::vec2(40.0, 36.0));
            plus_header(ui, |_| {});
        });
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
