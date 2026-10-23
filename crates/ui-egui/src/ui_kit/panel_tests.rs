use crate::ui_kit::test_support::*;
use crate::ui_kit::*;

use crate::{scenario, theme::Tokens};
use egui::Ui;

#[test]
fn checkbox_grid_and_close_header_are_clickable() {
    let ctx = crate::fonts::test_context();
    let (mut first, mut second, mut closed) = (false, true, false);
    let mut draw = |ui: &mut Ui| {
        header(ui, "extrude", "Extrude", || closed = true);
        checkbox_grid(ui, &mut [("First option", &mut first), ("Second option", &mut second)], 2);
    };
    frame(&ctx, vec![], &mut draw);
    let first_pos = scenario::handle_at("dialog:First option").unwrap();
    let second_pos = scenario::handle_at("dialog:Second option").unwrap();
    assert!((first_pos.y - second_pos.y).abs() < 0.01);
    assert!(second_pos.x > first_pos.x);
    frame(&ctx, pointer(first_pos, true), &mut draw);
    frame(&ctx, pointer(first_pos, false), &mut draw);
    let close = scenario::handle_at("dialog:close").unwrap();
    frame(&ctx, pointer(close, true), &mut draw);
    frame(&ctx, pointer(close, false), &mut draw);
    assert!(first && second && closed);
}

#[test]
fn heading_weight_field_surfaces_and_disclosures_follow_tokens_in_both_themes() {
    fn inspect(shape: &egui::Shape, t: Tokens, title: &mut usize, section: &mut usize, fields: &mut usize) {
        match shape {
            egui::Shape::Text(text) => {
                if text.galley.job.text == "3 mm" {
                    assert_eq!(text.galley.job.sections.first().unwrap().format.font_id.family, egui::FontFamily::Monospace);
                }
                if text.galley.job.text == "Fillet" {
                    assert_eq!(text.galley.job.sections.first().unwrap().format.font_id, crate::fonts::semibold(t.font_heading));
                    *title += 1;
                }
                if text.galley.job.text == "OPTIONS" {
                    let format = &text.galley.job.sections.first().unwrap().format;
                    assert_eq!(format.font_id, crate::fonts::semibold(t.font_caption));
                    assert!(format.extra_letter_spacing > 0.0);
                    *section += 1;
                }
            }
            egui::Shape::Rect(rect) if rect.fill == t.control_fill && rect.stroke.width == 1.0 && rect.stroke.color == t.control_border => {
                *fields += 1
            }
            _ => {}
        }
    }
    let previous = crate::theme::is_dark();
    for dark in [false, true] {
        crate::theme::set_dark(dark);
        let ctx = crate::fonts::test_context();
        ctx.set_visuals(if dark { egui::Visuals::dark() } else { egui::Visuals::light() });
        let mut value = "3 mm".to_string();
        let output = crate::test_support::frame(&ctx, egui::RawInput::default(), |ui| {
            ui.set_width(300.0);
            scope(ui, |ui| {
                header(ui, "fillet", "Fillet", || {});
                row(ui, "Radius", |ui| {
                    field(ui, &mut value);
                });
                section(ui, "options", "Options", true, |_| {});
            });
        });
        let (mut title, mut section, mut fields) = (0, 0, 0);
        for shape in &output.shapes {
            crate::test_support::each_shape(&shape.shape, &mut |shape| inspect(shape, Tokens::get(), &mut title, &mut section, &mut fields));
        }
        assert_eq!(scenario::rect_of("dialog:header").unwrap().height(), Tokens::get().form_header_height);
        assert_eq!(title, 1, "the semibold heading is painted once");
        assert_eq!(section, 1, "semibold small caps are letter spaced and painted once");
        assert!(fields > 0, "the actual text editor has a token field fill and one pixel border");
    }
    crate::theme::set_dark(previous);
}

#[test]
fn panel_keeps_actions_and_body_geometry_in_both_layouts() {
    for layout in [crate::shell::LayoutMode::Docked, crate::shell::LayoutMode::Floating] {
        let ctx = crate::fonts::test_context();
        let vp = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(640.0, 480.0));
        for _ in 0..3 {
            scenario::clear_handles();
            crate::test_support::frame(&ctx, egui::RawInput { screen_rect: Some(vp), ..Default::default() }, |_| {
                let side = crate::frame::Side::Right;
                let frame = Panel::feature_frame(&ctx, layout, side);
                let anchor = egui::pos2(vp.right() - crate::shell::SHELL_GAP, vp.top());
                let panel = Panel::new("Fillet", egui::Id::new("gallery_feature"), anchor, vp, side, frame);
                panel.window(&ctx).show(&ctx, |ui| {
                    panel.feature(
                        ui,
                        FeatureOptions {
                            icon: "fillet",
                            title: "Fillet",
                            drag_key: "gallery",
                            moved: egui::Vec2::ZERO,
                            docked: anchor,
                            apply_label: Some("Apply"),
                        },
                        false,
                        true,
                        |ui| {
                            let mut radius = "3 mm".to_owned();
                            row(ui, "Radius", |ui| {
                                field(ui, &mut radius);
                            });
                            ComputeStatus { text: None, busy: false, applying: false }
                        },
                    );
                });
            });
        }
        let header = scenario::rect_of("dialog:header").unwrap();
        let body = scenario::rect_of("dialog:row:Radius").unwrap();
        let footer = scenario::rect_of("dialog:footer").unwrap();
        assert_eq!(header.height(), Tokens::get().form_header_height);
        assert_eq!(body.height(), Tokens::get().control_height);
        assert_eq!(footer.height(), Tokens::get().form_footer_height);
        assert!(body.top() >= header.bottom());
        assert!(footer.top() >= body.bottom());
        assert!(vp.contains_rect(header) && vp.contains_rect(body) && vp.contains_rect(footer));
        assert!(footer.contains(scenario::handle_at("dialog:ok").unwrap()));
        assert!(footer.contains(scenario::handle_at("dialog:apply").unwrap()));
    }
}

#[test]
fn palette_scrolls_short_views_and_retains_its_status_and_action_handles() {
    for layout in [crate::shell::LayoutMode::Docked, crate::shell::LayoutMode::Floating] {
        for height in [125.0, 480.0] {
            let ctx = crate::fonts::test_context();
            let vp = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(640.0, height));
            for _ in 0..3 {
                scenario::clear_handles();
                crate::test_support::frame(&ctx, egui::RawInput { screen_rect: Some(vp), ..Default::default() }, |_| {
                    let frame = Panel::palette_frame(&ctx, layout);
                    let bottom = vp.bottom() - crate::shell::SHELL_GAP;
                    let panel = Panel::new(
                        "Options",
                        egui::Id::new("gallery_palette"),
                        egui::pos2(vp.right() - crate::shell::SHELL_GAP, 0.0),
                        vp,
                        crate::frame::Side::Right,
                        frame,
                    );
                    panel.window(&ctx).show(&ctx, |ui| {
                        panel.palette(ui, bottom, 0.0, |ui, part| match part {
                            Part::Header => {
                                status_header(
                                    ui,
                                    "sketch_palette",
                                    "Sketch 1",
                                    StatusHeader { icon: "sketch", text: "Fully constrained", complete: true, fraction: 1.0 },
                                );
                            }
                            Part::Body => {
                                checkbox_row(ui, "snap", "Snap", true);
                            }
                            Part::Footer => {
                                button_pair(
                                    ui,
                                    "sketch_palette",
                                    [false, false],
                                    [
                                        PairButton { key: "look_at", label: "ui.look_at", icon: Some("lookat") },
                                        PairButton { key: "slice", label: "Slice", icon: None },
                                    ],
                                );
                            }
                        });
                    });
                });
            }
            let scroll = scenario::rect_of("sketch_palette:options").unwrap();
            assert!(scroll.is_finite() && scroll.is_positive());
            assert!(vp.contains_rect(scroll));
            assert!(scenario::handle_at("sketch_palette:toggle:snap").is_some());
            assert!(scenario::handle_at("sketch_palette:look_at").is_some());
            assert!(scenario::handle_at("sketch_palette:slice").is_some());
        }
    }
}

#[test]
fn wide_tables_remain_bounded_with_pinned_actions_on_small_viewports() {
    for layout in [crate::shell::LayoutMode::Floating, crate::shell::LayoutMode::Docked] {
        for width in [240.0, 640.0, 1200.0] {
            let ctx = crate::fonts::test_context();
            let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 360.0));
            for _ in 0..3 {
                scenario::clear_handles();
                crate::test_support::frame(&ctx, egui::RawInput { screen_rect: Some(viewport), ..Default::default() }, |_| {
                    let side = crate::frame::Side::Right;
                    let anchor = viewport.right_top();
                    let panel = Panel::new("Table", egui::Id::new("wide"), anchor, viewport, side, Panel::feature_frame(&ctx, layout, side)).wide();
                    panel.window(&ctx).show(&ctx, |ui| {
                        panel.feature(
                            ui,
                            FeatureOptions {
                                icon: "fillet",
                                title: "Table",
                                drag_key: "wide",
                                moved: egui::Vec2::ZERO,
                                docked: anchor,
                                apply_label: None,
                            },
                            false,
                            true,
                            |ui| {
                                for _ in 0..30 {
                                    text(ui, "Table body", Text::Note("row"));
                                }
                                ComputeStatus { text: None, busy: false, applying: false }
                            },
                        );
                    });
                });
            }
            let header = scenario::rect_of("dialog:header").unwrap();
            let footer = scenario::rect_of("dialog:footer").unwrap();
            assert!(viewport.expand(1.0).contains_rect(header), "{width}: {header:?}");
            assert!(viewport.expand(1.0).contains_rect(footer), "{width}: {footer:?}");
        }
    }
}

#[test]
fn window_is_one_and_a_half_cards_wide_with_its_actions_in_both_themes() {
    let previous = crate::theme::is_dark();
    for dark in [false, true] {
        crate::theme::set_dark(dark);
        let ctx = crate::fonts::test_context();
        let t = Tokens::get();
        let mut action = Action::None;
        for _ in 0..3 {
            scenario::clear_handles();
            crate::test_support::frame(
                &ctx,
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0))),
                    focused: true,
                    ..Default::default()
                },
                |ui| {
                    action = Window::new(egui::Id::new("prompt_test"), "delete", "Delete")
                        .actions("Delete", "Cancel", Some("Keep"))
                        .show(ui.ctx(), true, |ui| {
                            text(ui, "Three features go with it.", Text::Report("body", false));
                        })
                        .1;
                },
            );
        }
        assert_eq!(action, Action::None);
        let header = scenario::rect_of("dialog:header").unwrap();
        let footer = scenario::rect_of("dialog:footer").unwrap();
        assert!((header.width() - (t.form_card_width * 1.5 - 2.0)).abs() <= 3.0, "{dark}: {header:?}");
        assert_eq!(header.width(), footer.width());
        assert!(footer.top() > header.bottom());
        let (ok, apply, cancel) =
            (scenario::rect_of("dialog:ok").unwrap(), scenario::rect_of("dialog:apply").unwrap(), scenario::rect_of("dialog:cancel").unwrap());
        assert!(cancel.right() < apply.left() && apply.right() < ok.left(), "Cancel, the third action, then the primary one");
        assert!(footer.contains_rect(ok) && footer.contains_rect(cancel));
        assert!(scenario::handle_at("dialog:Keep").is_some() && scenario::handle_at("dialog:close").is_some());
    }
    crate::theme::set_dark(previous);
}

#[test]
fn escape_closes_a_window_unless_it_keeps_the_card_open() {
    for keep in [false, true] {
        let ctx = crate::fonts::test_context();
        let mut action = Action::None;
        for frame in 0..3 {
            let events = if frame == 2 { crate::ui_kit::test_support::key(egui::Key::Escape) } else { Vec::new() };
            crate::test_support::frame(
                &ctx,
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0))),
                    focused: true,
                    events,
                    ..Default::default()
                },
                |ui| {
                    let prompt = Window::new(egui::Id::new(("prompt_escape", keep)), "settings", "Report").actions("Open", "Close", None);
                    let prompt = if keep { prompt.keep_open_on_escape() } else { prompt };
                    action = prompt
                        .show(ui.ctx(), true, |ui| {
                            text(ui, "A long text the user is editing.", Text::Report("body", false));
                        })
                        .1;
                },
            );
        }
        assert_eq!(action, if keep { Action::None } else { Action::Cancel }, "keep_open_on_escape={keep}");
    }
}

#[test]
fn window_pages_have_shared_metrics_callbacks_and_a_pinned_left_footer_action() {
    let previous = crate::theme::is_dark();
    for dark in [false, true] {
        crate::theme::set_dark(dark);
        let ctx = crate::fonts::test_context();
        let mut selected = usize::MAX;
        let mut drawn = 0;
        let pages = [
            WindowPage {
                group: None,
                icon: "settings",
                label: "ui.general",
                handle: "test:page:general",
                scroll: true,
                draw: |ui, state| {
                    *state = 1;
                    text(ui, "General", Text::Inline);
                },
            },
            WindowPage {
                group: None,
                icon: "orbit",
                label: "ui.navigation",
                handle: "test:page:navigation",
                scroll: true,
                draw: |ui, state| {
                    *state = 2;
                    text(ui, "Navigation", Text::Inline);
                },
            },
        ];
        for _ in 0..3 {
            scenario::clear_handles();
            crate::test_support::frame(
                &ctx,
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 800.0))),
                    focused: true,
                    ..Default::default()
                },
                |ui| {
                    Window::new(ui.id().with("pages"), "settings", "Preferences").cards(3.0).show_pages(
                        ui.ctx(),
                        &mut selected,
                        &pages,
                        &mut drawn,
                        |ui| {
                            button(ui, "Restore defaults", Button::Normal);
                        },
                    );
                },
            );
        }
        assert_eq!((selected, drawn), (1, 2), "hostile page index is bounded before callback dispatch");
        let nav = scenario::rect_of("dialog:pages").unwrap();
        let header = scenario::rect_of("dialog:header").unwrap();
        let footer = scenario::rect_of("dialog:footer").unwrap();
        assert_eq!(nav.width(), 188.0);
        assert_eq!(header.height(), 40.0);
        assert_eq!(footer.height(), 48.0);
        assert_eq!(nav.top(), header.bottom(), "no body inset around navigation");
        assert_eq!(scenario::rect_of("test:page:general").unwrap().height(), 32.0);
        assert!(footer.top() >= nav.bottom());
        let left = scenario::rect_of("dialog:button::Restore defaults").unwrap();
        let cancel = scenario::rect_of("dialog:cancel").unwrap();
        assert!(footer.contains_rect(left) && left.right() < cancel.left());
        let page = scenario::handle_at("test:page:general").unwrap();
        for down in [true, false] {
            crate::test_support::frame(
                &ctx,
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 800.0))),
                    focused: true,
                    events: pointer(page, down),
                    ..Default::default()
                },
                |ui| {
                    Window::new(ui.id().with("pages"), "settings", "Preferences").cards(3.0).show_pages(
                        ui.ctx(),
                        &mut selected,
                        &pages,
                        &mut drawn,
                        |_| {},
                    );
                },
            );
        }
        assert_eq!((selected, drawn), (0, 1), "click dispatches the declared page");
    }
    crate::theme::set_dark(previous);
}

#[test]
fn confirm_routes_enter_escape_selected_answers_and_blocks_background_clicks() {
    for (key, valid, selected, expected) in [
        (egui::Key::Enter, true, Action::Ok, Action::Ok),
        (egui::Key::Enter, false, Action::Ok, Action::None),
        (egui::Key::Enter, true, Action::Apply, Action::Apply),
        (egui::Key::Escape, true, Action::Ok, Action::Cancel),
    ] {
        let ctx = crate::fonts::test_context();
        let mut action = Action::None;
        let mut background = false;
        let mut at = egui::Pos2::ZERO;
        for pass in 0..5 {
            let events = match pass {
                2 => pointer(at, true),
                3 => pointer(at, false),
                4 => crate::test_support::key(key),
                _ => vec![],
            };
            scenario::clear_handles();
            crate::test_support::frame(
                &ctx,
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 700.0))),
                    focused: true,
                    events,
                    ..Default::default()
                },
                |ui| {
                    let button = button(ui, "Background", Button::Normal);
                    at = button.rect.center();
                    background |= button.clicked();
                    action = Confirm::new(ui.id().with("modal"), "status_warning", "Save changes?")
                        .tone(Tone::Warning)
                        .actions("Save", "Cancel", Some("Don't Save"))
                        .selected(selected)
                        .detail("Sample Plate")
                        .show(ui.ctx(), valid, |ui| {
                            text(ui, "Your changes will be lost.", Text::Report("message", false));
                        })
                        .1;
                },
            );
        }
        assert!(!background, "modal intercepts background pointer events");
        assert_eq!(action, expected, "{key:?}, valid={valid}, selected={selected:?}");
        assert_eq!(scenario::rect_of("confirm:disc").unwrap().size(), egui::vec2(36.0, 36.0));
        let (ghost, cancel, primary) =
            (scenario::rect_of("dialog:apply").unwrap(), scenario::rect_of("dialog:cancel").unwrap(), scenario::rect_of("dialog:ok").unwrap());
        assert!(ghost.right() < cancel.left() && cancel.right() < primary.left());
        assert!(scenario::rect_of("dialog:note:confirm_detail").is_some());
    }
}

#[test]
fn destructive_confirm_paints_error_primary_with_readable_on_colour_in_both_themes() {
    let previous = crate::theme::is_dark();
    for dark in [false, true] {
        crate::theme::set_dark(dark);
        let ctx = crate::fonts::test_context();
        let mut output = None;
        for _ in 0..3 {
            output = Some(crate::test_support::frame(
                &ctx,
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 700.0))),
                    focused: true,
                    ..Default::default()
                },
                |ui| {
                    Confirm::new(ui.id().with("delete"), "delete", "Delete").tone(Tone::Danger).actions("Delete", "Cancel", None).show(
                        ui.ctx(),
                        true,
                        |ui| {
                            text(ui, "These features will be deleted:", Text::Report("message", false));
                        },
                    );
                },
            ));
        }
        let t = Tokens::get();
        let primary = scenario::rect_of("dialog:ok").unwrap();
        let shapes = output.unwrap().shapes;
        let mut painted_fill = false;
        for shape in &shapes {
            crate::test_support::each_shape(&shape.shape, &mut |s| {
                painted_fill |= matches!(s, egui::Shape::Rect(r) if r.rect == primary && r.fill == t.form_error);
            });
        }
        assert!(painted_fill, "{dark}: primary must use form_error");
        let ink = if crate::theme::contrast_ratio(t.on_accent, t.form_error) >= 4.5 { t.on_accent } else { t.tab_on_fill_dark };
        assert!(crate::theme::contrast_ratio(ink, t.form_error) >= 4.5);
        let mut painted_ink = false;
        for shape in &shapes {
            crate::test_support::each_shape(&shape.shape, &mut |s| {
                painted_ink |= matches!(s, egui::Shape::Text(text) if primary.contains(text.pos) && text.galley.job.text == "Delete"
                    && text.galley.job.sections.iter().all(|section| section.format.color == ink));
            });
        }
        assert!(painted_ink, "{dark}: destructive primary text must use readable on-colour");
    }
    crate::theme::set_dark(previous);
}

#[test]
fn preferences_preserves_every_page_and_footer_in_both_themes() {
    let previous = crate::theme::is_dark();
    for dark in [false, true] {
        crate::theme::set_dark(dark);
        let mut h = crate::scenario::Harness::new();
        h.app.preferences.theme = if dark { "dark" } else { "light" }.into();
        h.app.home.open = false;
        for (index, page) in crate::prefs::PAGES.iter().enumerate() {
            crate::prefs::open_at(&mut h.app, index);
            h.frames(3);
            assert_eq!(h.app.prefs_window.section, index);
            let nav = scenario::rect_of("dialog:pages").unwrap();
            let row = scenario::rect_of(&format!("prefs:page:{page}")).unwrap();
            assert!(nav.contains_rect(row), "{dark}: {page} stays visible");
            let window = scenario::rect_of("prefs:window").unwrap();
            for action in ["prefs:ok", "prefs:apply", "prefs:cancel"] {
                assert!(window.contains_rect(scenario::rect_of(action).unwrap()), "{dark}: {page}: {action}");
            }
            assert!(scenario::rect_of("prefs:page_body").is_some());
            assert!(h.widget(h.app.tr("ui.restore_defaults")).is_some());
        }
    }
    crate::theme::set_dark(previous);
}

#[test]
fn window_footer_is_optional_and_can_contain_only_a_left_action() {
    for left in [false, true] {
        let ctx = crate::fonts::test_context();
        for _ in 0..3 {
            scenario::clear_handles();
            frame(&ctx, vec![], |ui| {
                let window = Window::new(ui.id().with("optional_footer"), "settings", "Preferences").actions("", "", None);
                if left {
                    window.show_with_left(
                        ui.ctx(),
                        true,
                        |_| {},
                        |ui| {
                            button(ui, "Restore defaults", Button::Normal);
                        },
                    );
                } else {
                    window.show(ui.ctx(), true, |_| {});
                }
            });
        }
        assert_eq!(scenario::rect_of("dialog:footer").is_some(), left);
        assert_eq!(scenario::rect_of("dialog:button::Restore defaults").is_some(), left);
        assert!(scenario::handle_at("dialog:ok").is_none() && scenario::handle_at("dialog:cancel").is_none());
        assert!(scenario::handle_at("dialog:close").is_some());
    }
}

#[test]
fn universal_containers_follow_one_layout_without_changing_transient_surfaces() {
    use crate::{frame::Side, shell::LayoutMode};
    let ctx = crate::fonts::test_context();
    let previous = crate::theme::is_dark();
    for dark in [false, true] {
        crate::theme::set_dark(dark);
        let t = Tokens::get();
        for surface in [Surface::Panel, Surface::Feature, Surface::Palette, Surface::Chrome] {
            LayoutMode::Floating.publish(&ctx);
            let floating = Container::new(&ctx, surface);
            assert!(floating.floating());
            let floating = floating.frame(&ctx, t.panel, Side::Left);
            assert_ne!(floating.shadow, egui::Shadow::NONE);
            assert!(floating.corner_radius.nw > 0);
            LayoutMode::Docked.publish(&ctx);
            let docked = Container::new(&ctx, surface);
            assert!(!docked.floating());
            assert_eq!(docked.gap(), 0.0);
            let docked = docked.frame(&ctx, t.panel, Side::Left);
            assert_eq!(docked.corner_radius.nw, 0, "{surface:?}: flush with left edge");
            assert_eq!(docked.fill, floating.fill);
            assert_eq!(docked.shadow, egui::Shadow::NONE, "compact surfaces meet their edge without shadows");
        }
        for surface in [
            Surface::ApplicationBar,
            Surface::Window,
            Surface::Popup,
            Surface::Canvas,
            Surface::Toast,
            Surface::Navigation,
            Surface::Viewport,
            Surface::InputShield,
        ] {
            LayoutMode::Floating.publish(&ctx);
            let floating = Container::new(&ctx, surface).frame(&ctx, t.panel, Side::Left);
            LayoutMode::Docked.publish(&ctx);
            let docked = Container::new(&ctx, surface).frame(&ctx, t.panel, Side::Left);
            assert_eq!(floating, docked, "{surface:?}: anchored/modal/canvas content never becomes a workspace dock");
        }
    }
    crate::theme::set_dark(previous);
}

#[test]
fn required_platform_window_keeps_recovery_pending_until_an_explicit_choice() {
    let ctx = crate::fonts::test_context();
    for _ in 0..3 {
        scenario::clear_handles();
        frame(&ctx, vec![], |ui| {
            assert!(!Window::new(ui.id().with("required_recovery"), "restore", "Recover unsaved work?").required().show_body(ui.ctx(), |ui| {
                button(ui, "Recover", Button::Normal);
                button(ui, "Discard", Button::Normal);
            }));
        });
    }
    assert!(scenario::handle_at("dialog:close").is_none(), "recovery cannot be dismissed through the header");
    assert!(scenario::rect_of("dialog:header").is_some());
    assert!(scenario::rect_of("dialog:footer").is_none());
}

#[test]
fn adjoining_compact_frames_share_one_stroke_and_only_exposed_corners_round() {
    use crate::ui_kit::DockEdges;
    use crate::{frame::Side, shell::LayoutMode};
    let ctx = crate::fonts::test_context();
    LayoutMode::Docked.publish(&ctx);
    let t = Tokens::get();
    let browser = Container::new(&ctx, Surface::Panel).joined(DockEdges { left: true, top: true, ..Default::default() });
    let frame = browser.frame(&ctx, t.panel, Side::Left);
    assert_eq!(frame.corner_radius, egui::CornerRadius { nw: 0, ne: 0, sw: 0, se: t.radius_lg });
    let joined = browser.joined(DockEdges { left: true, top: true, bottom: true, ..Default::default() });
    assert_eq!(joined.frame(&ctx, t.panel, Side::Left).corner_radius, egui::CornerRadius::ZERO);
    let a = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(100.0, 60.0));
    let allocation = frame.outer_rect(a);
    let below = egui::Rect::from_min_size(egui::pos2(a.left(), allocation.bottom()), a.size());
    let beside = egui::Rect::from_min_size(egui::pos2(allocation.right(), a.top()), a.size());
    assert_eq!(frame.outer_rect(below).top(), allocation.bottom(), "vertical allocations touch");
    assert_eq!(frame.outer_rect(beside).left(), allocation.right(), "horizontal allocations touch");
    assert_eq!(frame.widget_rect(a).bottom() - frame.stroke.width, frame.widget_rect(below).top(), "one shared horizontal border");
    assert_eq!(frame.widget_rect(a).right() - frame.stroke.width, frame.widget_rect(beside).left(), "one shared vertical border");
}

#[test]
fn compact_tool_sheet_corners_follow_its_right_edge_contact() {
    let ctx = crate::fonts::test_context();
    crate::shell::LayoutMode::Docked.publish(&ctx);
    let t = Tokens::get();
    let container = Container::new(&ctx, Surface::Feature);
    let frame = container.frame(&ctx, t.dialog_body, crate::frame::Side::Right);
    assert_eq!(container.tool_frame(frame, false).corner_radius, egui::CornerRadius::same(t.radius_lg));
    assert_eq!(container.tool_frame(frame, true).corner_radius, egui::CornerRadius { nw: t.radius_lg, ne: 0, sw: t.radius_lg, se: 0 });
}
