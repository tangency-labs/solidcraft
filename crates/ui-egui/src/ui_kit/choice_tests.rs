use crate::ui_kit::test_support::*;
use crate::ui_kit::*;

use crate::scenario;
use egui::Ui;

#[test]
fn segmented_allocates_only_its_reserved_row() {
    for layout in [egui::Layout::top_down(egui::Align::Min), egui::Layout::left_to_right(egui::Align::Center)] {
        for width in [160.0, 320.0, 640.0] {
            for enabled in [&[][..], &[true, false, true, false][..], &[false; 4][..]] {
                let ctx = crate::fonts::test_context();
                frame(&ctx, vec![], |ui| {
                    let bounds = egui::Rect::from_min_size(ui.cursor().min, egui::vec2(width, 120.0));
                    let builder = || egui::UiBuilder::new().max_rect(bounds).layout(layout);
                    let mut expected = ui.new_child(builder());
                    let mut actual = ui.new_child(builder());
                    expected.allocate_exact_size(egui::vec2(width, row_height()), egui::Sense::hover());
                    let mut selected = 0;
                    assert!(!segmented(&mut actual, &mut selected, &["A", "B", "C", "D"], Segments { enabled, ..Default::default() }));
                    assert_eq!(actual.min_rect(), expected.min_rect(), "segments must not expand the parent bounds");
                    assert_eq!(actual.cursor(), expected.cursor(), "segments must not advance the parent beyond the row");
                    let next = egui::vec2(24.0, 20.0);
                    assert_eq!(
                        actual.allocate_exact_size(next, egui::Sense::hover()).0,
                        expected.allocate_exact_size(next, egui::Sense::hover()).0,
                        "the following control must retain its allocation"
                    );
                });
            }
        }
    }
}

#[test]
fn segmented_click_respects_parent_and_option_availability() {
    for parent_enabled in [false, true] {
        // Missing availability entries retain the enabled default.
        for enabled in [&[][..], &[true][..], &[true, true][..], &[true, false][..]] {
            let ctx = crate::fonts::test_context();
            scenario::clear_handles();
            let mut selected = 0;
            let mut changed = false;
            let mut draw = |ui: &mut Ui| {
                if !parent_enabled {
                    ui.disable();
                }
                row(ui, "Availability", |ui| {
                    changed |= segmented(ui, &mut selected, &["A", "B"], Segments { enabled, ..Default::default() });
                });
            };
            frame(&ctx, vec![], &mut draw);
            let pos = scenario::handle_at("dialog:Availability:B").expect("disabled segments still publish targets");
            frame(&ctx, pointer(pos, true), &mut draw);
            frame(&ctx, pointer(pos, false), &mut draw);
            let should_change = parent_enabled && enabled.get(1).copied().unwrap_or(true);
            assert_eq!(selected, usize::from(should_change));
            assert_eq!(changed, should_change);
        }
    }
}

#[test]
fn segmented_availability_keeps_unsupported_options_visible_and_inert() {
    let ctx = crate::fonts::test_context();
    let mut selected = 1;
    let mut draw = |ui: &mut Ui| {
        row(ui, "Backdrop availability", |ui| {
            segmented(
                ui,
                &mut selected,
                &["World", "Colour", "Gradient", "Clear"],
                Segments { enabled: &[false, true, false, true], ..Default::default() },
            );
        });
    };
    frame(&ctx, vec![], &mut draw);
    for option in ["World", "Gradient"] {
        let pos = scenario::handle_at(&format!("dialog:Backdrop availability:{option}")).unwrap();
        frame(&ctx, pointer(pos, true), &mut draw);
        frame(&ctx, pointer(pos, false), &mut draw);
    }
    assert_eq!(selected, 1);
}

#[test]
fn segmented_click_changes_selection_and_keeps_all_choices_visible() {
    let ctx = crate::fonts::test_context();
    let mut selected = 0;
    let mut draw = |ui: &mut Ui| {
        row(ui, "Continuity", |ui| segmented(ui, &mut selected, &["G1", "G2", "G3"], Default::default()));
    };
    frame(&ctx, vec![], &mut draw);
    let pos = scenario::handle_at("dialog:Continuity:G2").unwrap();
    frame(&ctx, pointer(pos, true), &mut draw);
    frame(&ctx, pointer(pos, false), &mut draw);
    assert_eq!(selected, 1);
    let control = scenario::rect_of("dialog:control:Continuity").unwrap();
    for option in ["G1", "G2", "G3"] {
        assert!(control.contains(scenario::handle_at(&format!("dialog:Continuity:{option}")).unwrap()));
    }
}

/// A segment shows its option in the user's language, as the dropdown it falls back to does, and
/// keeps the option as passed in its handle.
#[test]
fn segments_show_their_options_translated() {
    let ctx = crate::fonts::test_context();
    let bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(600.0, 120.0));
    let _japanese = crate::i18n::scope("ja");
    let mut selected = 0;
    let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(bounds), ..Default::default() }, |ui| {
        row(ui, "Section", |ui| segmented(ui, &mut selected, &["Circular", "Square"], Default::default()));
    });
    output.textures_delta.clear();
    let painted = |text: &str| output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(t) if t.galley.job.text == text));
    for (option, shown) in [("Circular", "円形"), ("Square", "正方形")] {
        assert!(painted(shown) && !painted(option), "{option} shows as {shown}");
        assert!(scenario::handle_at(&format!("dialog:Section:{option}")).is_some(), "{option} keeps its handle");
    }
}

#[test]
fn optional_field_keeps_empty_default_and_options_use_two_columns() {
    let ctx = crate::fonts::test_context();
    let mut value = String::new();
    let (mut flip, mut copy) = (false, true);
    frame(&ctx, vec![], |ui| {
        row(ui, "Depth", |ui| hint_field(ui, &mut value, "to the next face"));
        options(ui, "options", &mut [("Flip", &mut flip), ("Create Copy", &mut copy)]);
    });
    assert!(value.is_empty(), "displaying a hint must not introduce a parameter");
    let first = scenario::handle_at("dialog:Flip").unwrap();
    let second = scenario::handle_at("dialog:Create Copy").unwrap();
    assert_eq!(first.y, second.y);
    assert!(second.x > first.x);
    let mut draw = |ui: &mut Ui| options(ui, "options", &mut [("Flip", &mut flip), ("Create Copy", &mut copy)]);
    frame(&ctx, vec![], &mut draw);
    let first = scenario::handle_at("dialog:Flip").unwrap();
    frame(&ctx, pointer(first, true), &mut draw);
    frame(&ctx, pointer(first, false), &mut draw);
    assert!(flip && copy);
}

/// A scrollbar that takes its width from the page, coming and going, narrows the row by that width; the
/// choice between the segments and the dropdown must not flip because of that, only because the row
/// really is too narrow.
#[test]
fn segments_stay_segments_when_a_scrollbar_appears_and_disappears() {
    let options = ["Origin", "View center", "Point under the cursor"];
    let solid = egui::style::ScrollStyle::solid();
    let scrollbar = solid.allocated_width();
    assert!(scrollbar > 0.0);
    let fresh = || {
        let ctx = crate::fonts::test_context();
        ctx.all_styles_mut(|style| style.spacing.scroll = solid);
        ctx
    };
    let form = |ctx: &egui::Context, width: f32| {
        scenario::clear_handles();
        let mut selected = 0;
        frame(ctx, vec![], &mut |ui: &mut Ui| {
            ui.allocate_ui(egui::vec2(width, 60.0), |ui| {
                row(ui, "Orbit around", |ui| segmented(ui, &mut selected, &options, Default::default()));
            });
        });
        scenario::rect_of("dialog:segments:Orbit around").is_some()
    };
    // The width where the segments just fit, found without any memory of an earlier frame being
    // anything but the generous start (so it is the width less one scrollbar).
    let strict = (200..1200).map(|w| w as f32).find(|&w| form(&fresh(), w) && form(&fresh(), w + scrollbar + 1.0)).expect("a width fits");
    // From wherever it starts, with and without the scrollbar's width taken off, the form holds.
    for start in [strict + scrollbar, strict + scrollbar + 3.0] {
        let ctx = fresh();
        for width in [start, start - scrollbar, start, start - scrollbar, start - scrollbar] {
            assert!(form(&ctx, width), "start {start}: segments at {width}");
        }
    }
    // Really too narrow (more than the scrollbar short), it is the dropdown, and it comes back when widened.
    let ctx = fresh();
    assert!(!form(&ctx, strict - 3.0 * scrollbar) && !form(&ctx, strict - 3.0 * scrollbar), "far too narrow: the dropdown");
    assert!(form(&ctx, strict + scrollbar + 3.0), "wide again: segments");
}

#[test]
fn beveled_segments_share_fill_corners_at_internal_joins() {
    use crate::theme::Tokens;
    use egui::{Color32, Shape};
    let mut t = Tokens::light();
    t.control_radius = 8;
    t.borders.bevel = Some(crate::theme::Bevel {
        width: 0.5,
        highlight: Color32::WHITE,
        light: Color32::LIGHT_GRAY,
        shadow: Color32::GRAY,
        dark_shadow: Color32::BLACK,
    });
    let _restore = crate::theme::scoped_tokens(t);
    for selected in 0..3 {
        let ctx = crate::fonts::test_context();
        let mut selected = selected;
        let output = crate::test_support::frame(&ctx, egui::RawInput::default(), |ui| {
            ui.set_width(300.0);
            segmented(ui, &mut selected, &["Left", "Middle", "Right"], Default::default());
        });
        let fill = output
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                Shape::Rect(r) if r.fill == t.selected_bg => Some(r),
                _ => None,
            })
            .expect("selected segment fill");
        for edge in [fill.rect.left(), fill.rect.right()] {
            let internal = if edge == fill.rect.left() { selected > 0 } else { selected < 2 };
            if internal {
                // A rounded bevel used to curve away while selection filled the square
                // internal corner. Its top band must now reach the segment join.
                assert!(output.shapes.iter().any(|s| match &s.shape {
                    Shape::LineSegment { points, .. } =>
                        points.iter().any(|p| (p.y - fill.rect.top() - 0.25).abs() < 0.01 && (p.x - edge).abs() <= 0.26),
                    _ => false,
                }));
            }
        }
    }
}

#[test]
fn stage_chips_reserve_the_selected_weight_for_complete_labels_in_all_five_catalogs() {
    let ctx = crate::fonts::test_context();
    let bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 120.0));
    for language in ["en", "ja", "zh", "de", "ko"] {
        let locale = crate::i18n::Locale::for_language(language);
        let labels: Vec<_> = ["render.stage_look", "render.stage_light", "render.stage_frame", "render.stage_output"]
            .iter()
            .map(|key| locale.text(key).to_string())
            .collect();
        for selected in 0..4 {
            let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(bounds), ..Default::default() }, |ui| {
                StageBar::row(ui, bounds, false, |ui| StageBar::header(ui, selected, &labels.iter().map(String::as_str).collect::<Vec<_>>(), &[]));
            });
            output.textures_delta.clear();
            let search = ctx.data(|d| d.get_temp::<egui::Rect>(egui::Id::new("sc_ribbon_search"))).unwrap();
            assert!((search.right() - (bounds.right() - crate::theme::Tokens::get().space_6)).abs() < 1.0);
            for label in &labels {
                assert!(
                    output
                        .shapes
                        .iter()
                        .any(|shape| { matches!(&shape.shape,egui::Shape::Text(text) if text.galley.rows.iter().any(|row|row.text()==*label)) }),
                    "{language}, selected {selected}: complete stage label {label}"
                );
            }
        }
    }
}

#[test]
fn thumbnail_choice_states_share_geometry_and_publish_modified_and_running_indicators() {
    let ctx = crate::fonts::test_context();
    let previous = crate::theme::is_dark();
    for dark in [false, true] {
        crate::theme::set_dark(dark);
        crate::theme::apply(&ctx);
        let mut dimensions = Vec::new();
        for (selected, modified, progress) in [(false, false, None), (true, false, None), (true, true, Some(0.4))] {
            scenario::clear_handles();
            frame(&ctx, vec![], |ui| {
                let response = thumbnail_choice(ui, "shot", "Hero", ThumbnailChoice { selected, modified, progress, ..Default::default() });
                dimensions.push(response.rect.size());
            });
            assert_eq!(scenario::rect_of("dialog:list:shot:modified").is_some(), modified);
            assert_eq!(scenario::rect_of("dialog:list:shot:progress").is_some(), progress.is_some());
        }
        assert!(dimensions.iter().all(|size| *size == dimensions[0]));
        assert_eq!(dimensions[0].y, 38.0);
    }
    crate::theme::set_dark(previous);
}

#[test]
fn thumbnail_choices_with_ready_images_preserve_names_and_full_chip_stride() {
    let ctx = crate::fonts::test_context();
    let texture = ctx.load_texture("shot", egui::ColorImage::filled([104, 60], egui::Color32::GRAY), Default::default());
    frame(&ctx, vec![], |ui| {
        ui.horizontal(|ui| {
            let first = thumbnail_choice(ui, "detail", "Detail", ThumbnailChoice { texture: Some(texture.id()), ..Default::default() });
            let second = thumbnail_choice(ui, "front", "Front", ThumbnailChoice { texture: Some(texture.id()), ..Default::default() });
            assert!(first.rect.right() <= second.rect.left(), "loaded images must not move the cursor back over the preceding name");
            assert!(first.rect.width() > 52.0 + 24.0, "reserve the shot name beside the thumbnail");
        });
    });
}

#[test]
fn save_shot_dashes_leave_rounded_corners_at_the_chip_radius() {
    fn endpoints(shape: &egui::Shape, color: egui::Color32, points: &mut Vec<egui::Pos2>) {
        match shape {
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    endpoints(shape, color, points);
                }
            }
            egui::Shape::LineSegment { points: line, stroke } if stroke.color == color => points.extend(line),
            egui::Shape::Path(path) if matches!(path.stroke.color, egui::epaint::ColorMode::Solid(ink) if ink == color) => {
                points.extend(&path.points)
            }
            _ => {}
        }
    }
    let ctx = crate::fonts::test_context();
    let mut rect = egui::Rect::NOTHING;
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        rect = add_choice(ui, "Save Shot").rect;
    });
    let t = crate::theme::Tokens::get();
    let mut points = Vec::new();
    for shape in &output.shapes {
        endpoints(&shape.shape, t.control_border, &mut points);
    }
    assert!(!points.is_empty());
    let border = rect.shrink(t.space_2);
    for corner in [border.left_top(), border.right_top(), border.right_bottom(), border.left_bottom()] {
        assert!(points.iter().all(|point| point.distance(corner) > f32::from(t.control_radius) / 4.0), "dashes must follow the rounded perimeter");
    }
    output.textures_delta.clear();
}

#[test]
fn segmented_unavailable_choices_remain_visible_and_cannot_change_selection() {
    let ctx = crate::fonts::test_context();
    let mut selected = 1;
    let mut draw = |ui: &mut Ui| {
        row(ui, "Quality", |ui| {
            segmented(
                ui,
                &mut selected,
                &["Draft", "Custom"],
                Segments { disabled_reasons: &[Some("waiting for the quality contract"), None], ..Default::default() },
            )
        });
    };
    frame(&ctx, vec![], &mut draw);
    let draft = scenario::handle_at("dialog:Quality:Draft").unwrap();
    frame(&ctx, pointer(draft, true), &mut draw);
    frame(&ctx, pointer(draft, false), &mut draw);
    assert_eq!(selected, 1);
    assert!(scenario::handle_at("dialog:Quality:Custom").is_some());
}

#[test]
fn output_radio_rows_keep_titles_summaries_and_disabled_choices_in_both_themes() {
    for dark in [false, true] {
        let tokens = if dark { crate::theme::Tokens::dark() } else { crate::theme::Tokens::light() };
        let _restore = crate::theme::scoped_tokens(tokens);
        for language in ["en", "ja", "zh", "de", "ko"] {
            let locale = crate::i18n::Locale::for_language(language);
            let ctx = crate::fonts::test_context();
            let mut enabled = true;
            let mut clicked = false;
            let mut draw = |ui: &mut Ui| {
                ui.set_width(240.0);
                let response = ui
                    .add_enabled_ui(enabled, |ui| {
                        settings_choice_row(
                            ui,
                            &SettingsChoice {
                                key: "test:output:radio",
                                title: locale.text("render.size_full_hd"),
                                summary: "1920 × 1080",
                                detail: "Full HD",
                            },
                            true,
                        )
                    })
                    .inner;
                assert_eq!(response.rect.height(), 40.0);
                assert_eq!(response.rect.width(), 240.0);
                clicked |= response.clicked();
            };
            let output = crate::test_support::frame(&ctx, egui::RawInput::default(), &mut draw);
            let texts: Vec<_> = crate::test_support::text_shapes(&output).collect();
            let title = texts.iter().find(|text| text.galley.text() == locale.text("render.size_full_hd")).unwrap();
            let summary = texts.iter().find(|text| text.galley.text() == "1920 × 1080").unwrap();
            assert!(title.pos.x + title.galley.size().x <= summary.pos.x, "{language}: title must clear its summary");
            frame(&ctx, vec![], &mut draw);
            let point = scenario::handle_at("test:output:radio").unwrap();
            frame(&ctx, pointer(point, true), &mut draw);
            frame(&ctx, pointer(point, false), &mut draw);
            assert!(clicked);
            enabled = false;
            clicked = false;
            let mut disabled = |ui: &mut Ui| {
                ui.add_enabled_ui(enabled, |ui| {
                    clicked |= settings_choice_row(
                        ui,
                        &SettingsChoice { key: "test:output:radio", title: "4K", summary: "3840 × 2160", detail: "Over budget" },
                        false,
                    )
                    .clicked();
                });
            };
            frame(&ctx, vec![], &mut disabled);
            let point = scenario::handle_at("test:output:radio").unwrap();
            frame(&ctx, pointer(point, true), &mut disabled);
            frame(&ctx, pointer(point, false), &mut disabled);
            assert!(!clicked);
        }
    }
}

#[test]
fn segmented_space_and_arrows_choose_while_enter_applies_the_dialog() {
    for icons in [false, true] {
        let ctx = crate::fonts::test_context();
        let selected = std::cell::Cell::new(0);
        let target = std::cell::Cell::new(egui::Id::NULL);
        let applied = std::cell::Cell::new(false);
        let mut draw = |ui: &mut Ui| {
            crate::dialog_focus::prepare_active(ui.ctx());
            let (_, action) = Window::new(egui::Id::new("segment_dialog"), "", "Test").show(ui.ctx(), true, |ui| {
                row(ui, "Continuity", |ui| {
                    target.set(ui.make_persistent_id((if icons { "icon_segment" } else { "segment" }, 1usize)));
                    let mut value = selected.get();
                    if icons {
                        segmented_icons(
                            ui,
                            &mut value,
                            &[IconSegment { key: "G1", icon: "line", tip: "G1".into() }, IconSegment { key: "G2", icon: "circle", tip: "G2".into() }],
                            &[],
                        );
                    } else {
                        segmented(ui, &mut value, &["G1", "G2"], Segments { compact: true, ..Default::default() });
                    }
                    selected.set(value);
                });
            });
            if action == Action::Ok {
                applied.set(true);
            }
        };
        for _ in 0..3 {
            frame(&ctx, vec![], &mut draw);
        }
        ctx.memory_mut(|m| m.request_focus(target.get()));
        frame(&ctx, key(egui::Key::Enter), &mut draw);
        assert_eq!(selected.get(), 0, "Enter must preserve the choice (icons={icons})");
        assert!(applied.get(), "Enter must apply the dialog (icons={icons})");
        applied.set(false);
        ctx.memory_mut(|m| m.request_focus(target.get()));
        frame(&ctx, key(egui::Key::Space), &mut draw);
        assert_eq!(selected.get(), 1);
        assert!(!applied.get(), "Space selects without applying");
        for (arrow, expected) in [(egui::Key::ArrowLeft, 0), (egui::Key::ArrowRight, 1), (egui::Key::ArrowUp, 0), (egui::Key::ArrowDown, 1)] {
            frame(&ctx, key(arrow), &mut draw);
            assert_eq!(selected.get(), expected, "{arrow:?} must choose the adjacent option (icons={icons})");
            assert!(!applied.get());
        }
    }
}

#[test]
fn segmented_arrows_skip_disabled_options_and_stay_in_the_group() {
    let ctx = crate::fonts::test_context();
    let selected = std::cell::Cell::new(0);
    let first = std::cell::Cell::new(egui::Id::NULL);
    let mut draw = |ui: &mut Ui| {
        row(ui, "Continuity", |ui| {
            first.set(ui.make_persistent_id(("segment", 0usize)));
            let mut value = selected.get();
            segmented(ui, &mut value, &["G1", "G2", "G3"], Segments { enabled: &[true, false, true], compact: true, ..Default::default() });
            selected.set(value);
        });
    };
    for _ in 0..3 {
        frame(&ctx, vec![], &mut draw);
    }
    ctx.memory_mut(|m| m.request_focus(first.get()));
    frame(&ctx, key(egui::Key::ArrowRight), &mut draw);
    assert_eq!(selected.get(), 2, "arrows skip disabled G2");
    frame(&ctx, key(egui::Key::ArrowLeft), &mut draw);
    assert_eq!(selected.get(), 0);
    frame(&ctx, key(egui::Key::ArrowLeft), &mut draw);
    assert_eq!(selected.get(), 0);
    assert_eq!(ctx.memory(|m| m.focused()), Some(first.get()), "arrows at a boundary keep focus in the group");
}
