//! SolidCraft's specimens of the kit: the elements drawn from SolidCraft's data (its menus
//! from `context_menu`, the history strip and browser of a sample design, the continuity rows
//! of its surface dialogs) and the boards that need a running app. The kit's own gallery,
//! drawn from plain data, is `ui_kit::gallery`; these are checked with its `check_elements`.
use crate::dialogs_surface::{continuity_row, continuity_row_explained, continuity_tip};
use crate::ui_kit::element;
use crate::ui_kit::gallery::{ELEMENTS, Element, Sample, check_elements, example};
use crate::ui_kit::*;
use egui::{Ui, vec2};
use solidcraft_engine::kernel::Continuity;
use solidcraft_theme::Tokens;

const APP_ELEMENTS: &[Element] = &[
    element!("history_icons", |ui, _| history_example(ui, false)),
    element!("history_names", |ui, _| history_example(ui, true)),
    element!("radial_menu", |ui, _| {
        let (r, _) = ui.allocate_exact_size(vec2(360.0, 360.0), egui::Sense::hover());
        let mut app = crate::test_support::sample_app();
        app.ui.tab = "SURFACE".into();
        app.publish_tab_accent(ui.ctx());
        radial_menu(ui, r.center(), &crate::context_menu::radial_items(&app), Some(3));
    }),
    element!("workspace_menu", |ui, _| {
        let app = crate::test_support::sample_app();
        crate::context_menu::workspace_menu(ui, &crate::context_menu::items(&app, &crate::context_menu::Target::Workspace), None, 550.0);
    }),
    element!("continuity_row", "dialog:row:Gallery", Some("dialog:Gallery"), |ui, _| {
        let mut order = Continuity::G1;
        continuity_row(ui, "Gallery", "gallery_continuity", &mut order, &[Continuity::G0, Continuity::G1, Continuity::G2]);
    }),
    element!("continuity_row_explained", "dialog:row:Gallery", Some("dialog:Gallery"), |ui, _| {
        let mut order = Continuity::G2;
        let allowed = [Continuity::G1, Continuity::G2];
        continuity_row_explained(ui, "Gallery", "gallery_continuity", &mut order, &allowed, &continuity_tip(&allowed));
    }),
    element!("preference_specimens", |ui, _| {
        crate::prefs::figures::scale_specimen(ui, 1.1);
        crate::prefs::figures::status_palette_sample(ui, solidcraft_theme::ColorVision::Deutan);
        crate::prefs::figures::colour_vision_sample(ui, &Tokens::get());
        crate::prefs::figures::autosave_timeline(ui, 5.0);
        crate::prefs::figures::pointer_preview(ui, true, "Normal", true);
    }),
    element!("visual_style_dropdown", "dialog:row:Visual style", Some("dialog:Visual style"), |ui, s| {
        row(ui, "Visual style", |ui| {
            let options: Vec<_> = crate::navigation_slots::VISUAL_STYLES
                .iter()
                .enumerate()
                .map(|(index, (_, label))| (*label, crate::navigation_slots::visual_style_icon(index as u8)))
                .collect();
            dropdown_icons(ui, "gallery_visual_style", &mut s.selected, &options);
        });
    }),
    element!("continuity_tip", "dialog:row:Gallery", None, |ui, _| {
        let tip = continuity_tip(&[Continuity::G0, Continuity::G1]);
        row(ui, "Gallery", |ui| ui.label(tip));
    }),
];

#[test]
fn app_elements_have_geometry_and_stay_in_their_allocation_in_both_themes() {
    check_elements(APP_ELEMENTS, crate::fonts::test_context);
}

#[test]
fn visual_style_dropdown_specimen_selects_real_options_and_paints_their_icons() {
    let previous = solidcraft_theme::is_dark();
    let element = APP_ELEMENTS.iter().find(|element| element.name == "visual_style_dropdown").unwrap();
    for dark in [false, true] {
        solidcraft_theme::set_dark(dark);
        let ctx = crate::fonts::test_context();
        let sample = std::cell::RefCell::new(Sample { value: String::new(), second: String::new(), selected: 0, checked: false });
        let mut draw = |ui: &mut Ui| {
            (element.draw)(ui, &mut sample.borrow_mut());
        };
        for _ in 0..3 {
            crate::ui_kit::test_support::frame(&ctx, vec![], &mut draw);
        }
        for (index, (_, label)) in crate::navigation_slots::VISUAL_STYLES.into_iter().enumerate() {
            let trigger = solidcraft_ui_registry::handle_at("dialog:Visual style").unwrap();
            crate::ui_kit::test_support::frame(&ctx, crate::ui_kit::test_support::pointer(trigger, true), &mut draw);
            crate::ui_kit::test_support::frame(&ctx, crate::ui_kit::test_support::pointer(trigger, false), &mut draw);
            crate::ui_kit::test_support::frame(&ctx, vec![], &mut draw);
            for (option, (_, name)) in crate::navigation_slots::VISUAL_STYLES.into_iter().enumerate() {
                let rect = solidcraft_ui_registry::rect_of(&format!("dialog:Visual style:{name}")).unwrap();
                let icon = crate::navigation_slots::visual_style_icon(option as u8);
                let painted = solidcraft_ui_registry::rect_of(&format!("dialog:dropdown_icon:{icon}")).unwrap();
                let owner =
                    if option == sample.borrow().selected { solidcraft_ui_registry::rect_of("dialog:widget:Visual style").unwrap() } else { rect };
                assert!(owner.contains_rect(painted), "{name}: its real icon is in its option or selected trigger");
            }
            let option = solidcraft_ui_registry::handle_at(&format!("dialog:Visual style:{label}")).unwrap();
            crate::ui_kit::test_support::frame(&ctx, crate::ui_kit::test_support::pointer(option, true), &mut draw);
            crate::ui_kit::test_support::frame(&ctx, crate::ui_kit::test_support::pointer(option, false), &mut draw);
            crate::ui_kit::test_support::frame(&ctx, vec![], &mut draw);
            assert_eq!(sample.borrow().selected, index, "the specimen edits the real selection index");
            let icon = crate::navigation_slots::visual_style_icon(index as u8);
            let trigger = solidcraft_ui_registry::rect_of("dialog:widget:Visual style").unwrap();
            assert!(trigger.contains_rect(solidcraft_ui_registry::rect_of(&format!("dialog:dropdown_icon:{icon}")).unwrap()));
            assert!(solidcraft_ui_registry::handle_at("dialog:Visual style:Wireframe").is_none(), "the native popup closes after selection");
        }
    }
    solidcraft_theme::set_dark(previous);
}

#[cfg(test)]
fn history_example(ui: &mut Ui, names: bool) {
    let mut app = crate::test_support::sample_app();
    app.preferences.timeline_left = true;
    app.preferences.timeline_left_width = crate::ui_kit::history_width(names);
    app.run("timeline.roll_to", serde_json::json!({"position":3})).unwrap();
    let feature = app.session.doc.features.get(2).unwrap().id;
    app.run("select.set", serde_json::json!({"items":[{"type":"feature","id":feature}]})).unwrap();
    let width = crate::timeline_left::strip_width(&app);
    let rect = ui.allocate_space(vec2(width, 650.0)).1;
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect.shrink(5.0)), |ui| {
        let frame = Container::new(ui.ctx(), Surface::Panel).frame(ui.ctx(), Tokens::get().panel, crate::frame::Side::Left);
        Container::paint_frame(ui.painter(), frame, rect);
        crate::timeline_left::left_timeline(&mut app, ui);
    });
}

/// CPU captures of the actual vertical strip and browser, in both themes and widths.
#[test]
fn history_board_in_both_themes_and_widths() {
    use crate::ui_kit::{test_raster as raster, test_textures_update as textures_update};
    use std::collections::HashMap;
    let previous = solidcraft_theme::is_dark();
    for dark in [false, true] {
        for names in [false, true] {
            for collapsed in [false, true] {
                for state in [HistoryHandleState::Rest, HistoryHandleState::Hover, HistoryHandleState::Drag] {
                    solidcraft_theme::set_dark(dark);
                    let ctx = crate::fonts::test_context();
                    ctx.set_visuals(if dark { egui::Visuals::dark() } else { egui::Visuals::light() });
                    ctx.global_style_mut(|style| style.animation_time = 0.0);
                    let mut app = crate::test_support::sample_app();
                    app.preferences.timeline_left = true;
                    app.preferences.timeline_left_width = crate::ui_kit::history_width(names);
                    app.preferences.timeline_left_browser_collapsed = collapsed;
                    app.run("timeline.roll_to", serde_json::json!({"position":3})).unwrap();
                    let feature = app.session.doc.features.get(2).unwrap().id;
                    app.run("select.set", serde_json::json!({"items":[{"type":"feature","id":feature}]})).unwrap();
                    let bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(560.0, 680.0));
                    let mut textures = HashMap::new();
                    let mut image = None;
                    let mut drag_start = None;
                    for pass in 0..9 {
                        let mut events = vec![];
                        if pass >= 2 {
                            let card = app.shell.history.unwrap();
                            if state == HistoryHandleState::Hover {
                                events.push(egui::Event::PointerMoved(card.right_center()));
                            } else if state == HistoryHandleState::Drag {
                                let start = *drag_start.get_or_insert(egui::pos2(card.right() - 4.0, card.top() + 12.0));
                                if pass == 2 {
                                    events = crate::test_support::pointer(start, true);
                                } else if pass == 3 {
                                    events.push(egui::Event::PointerMoved(start + vec2(112.0 - history_width(names), 0.0)));
                                }
                            }
                        }
                        let mut output = ctx.run_ui(
                            egui::RawInput { screen_rect: Some(bounds), time: Some(pass as f64 * 0.2), focused: true, events, ..Default::default() },
                            |ui| {
                                ui.painter().rect_filled(bounds, 0.0, Tokens::get().viewport_top);
                                crate::timeline_left::left_panel(&mut app, ui);
                                crate::browser::browser(&mut app, ui);
                            },
                        );
                        textures_update(&mut textures, &output.textures_delta);
                        output.textures_delta.clear();
                        image = Some(raster(&ctx, output.shapes, output.pixels_per_point, bounds, &textures));
                    }
                    if state == HistoryHandleState::Drag {
                        assert_eq!(app.shell.history_drag_width, Some(112.0));
                        assert_eq!(app.shell.history.unwrap().width(), 112.0);
                    }
                    assert!(app.shell.history.unwrap().right() < app.shell.browser.unwrap().left());
                    if let Ok(dir) = std::env::var("SOLIDCRAFT_HISTORY_EVIDENCE") {
                        std::fs::create_dir_all(&dir).unwrap();
                        image
                            .unwrap()
                            .save(std::path::Path::new(&dir).join(format!(
                                "history-{}-{}-{}-{:?}.png",
                                if names { "names" } else { "icons" },
                                if collapsed { "rail" } else { "browser" },
                                if dark { "dark" } else { "light" },
                                state
                            )))
                            .unwrap();
                    }
                }
            }
        }
    }
    solidcraft_theme::set_dark(previous);
}

/// Render actual egui geometry without a desktop window. Opt-in PNGs are reviewer evidence.
#[test]
fn menu_board_in_both_themes() {
    use crate::ui_kit::{test_raster as raster, test_textures_update as textures_update};
    use std::collections::HashMap;
    let previous = solidcraft_theme::is_dark();
    for dark in [false, true] {
        solidcraft_theme::set_dark(dark);
        let ctx = crate::fonts::test_context();
        ctx.set_visuals(if dark { egui::Visuals::dark() } else { egui::Visuals::light() });
        let mut textures = HashMap::new();
        let bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(1100.0, 760.0));
        let mut app = crate::test_support::sample_app();
        app.last_command = Some(("solid.extrude".into(), "Extrude".into()));
        let items = crate::context_menu::radial_items(&app);
        let mut image = None;
        for _ in 0..3 {
            let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(bounds), ..Default::default() }, |ui| {
                ui.painter().rect_filled(bounds, 0.0, Tokens::get().panel);
                for (name, at) in
                    [("menu", egui::pos2(30.0, 30.0)), ("menu_submenu", egui::pos2(30.0, 430.0)), ("rename_menu", egui::pos2(630.0, 430.0))]
                {
                    ui.scope_builder(egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(at, vec2(550.0, 350.0))), |ui| {
                        example(ui, ELEMENTS.iter().find(|e| e.name == name).unwrap());
                    });
                }
                ui.allocate_rect(egui::Rect::from_min_size(egui::pos2(630.0, 30.0), vec2(360.0, 360.0)), egui::Sense::hover());
                radial_menu(ui, egui::pos2(810.0, 210.0), &items, Some(3));
            });
            textures_update(&mut textures, &output.textures_delta);
            output.textures_delta.clear();
            image = Some(raster(&ctx, output.shapes, output.pixels_per_point, bounds, &textures));
        }
        let image = image.unwrap();
        assert!(image.pixels().any(|p| p.0[3] != 0));
        if let Ok(dir) = std::env::var("SOLIDCRAFT_MENU_EVIDENCE") {
            std::fs::create_dir_all(&dir).unwrap();
            image.save(std::path::Path::new(&dir).join(if dark { "menus-dark.png" } else { "menus-light.png" })).unwrap();
        }
    }
    solidcraft_theme::set_dark(previous);
}

/// CPU raster of the actual popup in all five board states. No display or GPU required.
#[test]
fn command_search_board_in_both_themes() {
    use crate::ui_kit::{test_raster as raster, test_textures_update as textures_update};
    use std::collections::HashMap;
    let previous = solidcraft_theme::is_dark();
    for dark in [false, true] {
        solidcraft_theme::set_dark(dark);
        let t = Tokens::get();
        assert!(solidcraft_theme::contrast_ratio(t.form_success, t.form_success_fill) >= 4.5);
        assert!(solidcraft_theme::contrast_ratio(t.selected_ink, t.accent_soft) >= 4.5);
        for (state, query) in [
            ("empty", ""),
            ("matches", "fillet"),
            ("command", r#"solid.extrude {"distance":20}"#),
            ("invalid", r#"solid.extrude {"distance":}"#),
            ("no-match", "filletttt"),
        ] {
            let ctx = crate::fonts::test_context();
            ctx.set_visuals(if dark { egui::Visuals::dark() } else { egui::Visuals::light() });
            let mut app = crate::test_support::sample_app();
            app.run("parameters.add", serde_json::json!({"name":"fillet_radius","expression":"6 mm"})).unwrap();
            app.sbox.used("solid.extrude");
            app.sbox.used("solid.fillet");
            app.sbox.used("sketch.create");
            crate::shortcut_box::open(&mut app, Some(egui::pos2(20.0, 20.0)));
            app.sbox.text = query.to_owned();
            let bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(570.0, 650.0));
            let mut textures = HashMap::new();
            let mut image = None;
            for _ in 0..3 {
                solidcraft_ui_registry::clear_handles();
                let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(bounds), focused: true, ..Default::default() }, |ui| {
                    ui.painter().rect_filled(bounds, 0.0, Tokens::get().panel);
                    crate::shortcut_box::show(&mut app, ui.ctx());
                });
                textures_update(&mut textures, &output.textures_delta);
                output.textures_delta.clear();
                image = Some(raster(&ctx, output.shapes, output.pixels_per_point, bounds, &textures));
            }
            let popup = solidcraft_ui_registry::rect_of("ribbon:search_popup").unwrap();
            let field = solidcraft_ui_registry::rect_of("shortcut:search").unwrap();
            assert_eq!(field.height(), 40.0);
            assert_eq!(field.width(), 520.0);
            assert_eq!(popup.width(), 520.0);
            assert_eq!(popup.top(), field.bottom() + Tokens::get().space_6);
            assert_eq!(popup.x_range(), field.x_range());
            let footer = solidcraft_ui_registry::rect_of("ribbon:search_footer").unwrap();
            assert_eq!(footer, egui::Rect::from_min_max(egui::pos2(popup.left(), footer.top()), popup.right_bottom()));
            assert!(bounds.contains_rect(popup), "{state}: {popup:?}");
            match state {
                "matches" => {
                    assert!(solidcraft_ui_registry::handle_at("menu:solid.fillet").is_some());
                    assert_eq!(solidcraft_ui_registry::rect_of("menu:solid.fillet").unwrap().height(), 38.0);
                    assert!(solidcraft_ui_registry::handle_at("menu:search:design:0").is_some());
                }
                "command" => assert!(solidcraft_ui_registry::rect_of("dialog:note:command_parameters").is_some()),
                "invalid" => assert!(solidcraft_ui_registry::rect_of("dialog:report:command_parameters").is_some()),
                "no-match" => assert!(solidcraft_ui_registry::rect_of("dialog:report:search_no_match").is_some()),
                _ => {}
            }
            let image = image.unwrap();
            assert!(image.pixels().any(|p| p.0[3] != 0));
            if let Ok(dir) = std::env::var("SOLIDCRAFT_SEARCH_EVIDENCE") {
                std::fs::create_dir_all(&dir).unwrap();
                image.save(std::path::Path::new(&dir).join(format!("search-{state}-{}.png", if dark { "dark" } else { "light" }))).unwrap();
            }
        }
    }
    solidcraft_theme::set_dark(previous);
}

/// Exercise the actual surface painters, including deliberately shared artwork whose
/// own accent differs from its tone, tab or feature kind.
#[test]
fn confirm_radial_and_history_rows_keep_multitone_artwork() {
    let previous = solidcraft_theme::is_dark();
    for dark in [false, true] {
        solidcraft_theme::set_dark(dark);
        let mut cases = vec![
            ("warning", "extrude", "", "MODEL", Some("construction"), 20),
            ("danger", "extrude", "", "MODEL", Some("constraint"), 20),
            ("info", "extrude", "", "MODEL", None, 20),
            ("warning", "status_warning", "", "MODEL", Some("construction"), 20),
            ("danger", "delete", "", "MODEL", Some("constraint"), 20),
            ("danger_menu", "delete", "", "MODEL", Some("constraint"), 16),
            ("info", "surface_extrude", "", "MODEL", None, 20),
        ];
        for (tab, ramp) in [
            ("SKETCH", Some("sketch")),
            ("SURFACE", Some("surface")),
            ("MESH", Some("mesh")),
            ("FORM", Some("form")),
            ("MODEL", None),
            ("SHEET METAL", None),
            ("RENDER", None),
            ("unknown", None),
        ] {
            for surface in ["radial", "radial_hover"] {
                cases.push((surface, "extrude", "", tab, ramp, 24));
            }
        }
        for (kind, ramp) in [
            ("Sketch", Some("sketch")),
            ("SurfaceExtrudeFeature", Some("surface")),
            ("MeshFeature", Some("mesh")),
            ("FormFeature", Some("form")),
            ("ExtrudeFeature", None),
        ] {
            for (surface, side) in [("history_names", 15), ("history_icons", 19)] {
                cases.push((surface, "extrude", kind, "MODEL", ramp, side));
            }
        }
        // Surface feature names without the Surface prefix use the same alias rule
        // as the browser's timeline::icon_of assignments.
        cases.push(("history_names", "surface_extrude", "ExtrudeFeature", "MODEL", Some("surface"), 15));
        cases.push(("history_names", "continuity", "MatchFeature", "MODEL", Some("surface"), 15));
        let mut dominant_accents: std::collections::HashMap<&str, std::collections::HashSet<egui::Color32>> = std::collections::HashMap::new();
        for (surface, icon, kind, tab, ramp, side) in cases {
            let ctx = crate::fonts::test_context();
            ctx.set_visuals(if dark { egui::Visuals::dark() } else { egui::Visuals::light() });
            let mut app = crate::SolveApp::new(Default::default(), Default::default());
            app.ui.tab = tab.into();
            app.publish_tab_accent(&ctx);
            let bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(600.0, 360.0));
            let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(bounds), ..Default::default() }, |ui| {
                ui.painter().rect_filled(bounds, 0.0, Tokens::get().panel);
                match surface {
                    "radial" | "radial_hover" => {
                        radial_menu(
                            ui,
                            egui::pos2(180.0, 180.0),
                            &[crate::ui_kit::Item::action("solid.extrude", "Extrude", icon)],
                            (surface == "radial_hover").then_some(0),
                        );
                    }
                    "danger_menu" => {
                        let mut item = MenuItem::new("gallery:danger", "Delete", icon);
                        item.danger = true;
                        item.show(ui);
                    }
                    "history_names" | "history_icons" => {
                        let style = if surface == "history_names" { HistoryStyle::Names } else { HistoryStyle::Icons };
                        HistoryChip {
                            icon,
                            name: "Extrude1",
                            icon_accent: crate::workspace::feature_icon_accent(kind, icon),
                            selected: false,
                            hovered: false,
                            dim: false,
                            suppressed: false,
                            error: false,
                            warning: false,
                            reference: false,
                            accent: crate::workspace::tab_accent(&Tokens::get(), "MODEL"),
                            ink: None,
                        }
                        .paint(
                            ui.painter(),
                            egui::Rect::from_min_size(egui::pos2(20.0, 20.0), vec2(158.0, style.height())),
                            style,
                        );
                    }
                    _ => {
                        let tone = match surface {
                            "warning" => Tone::Warning,
                            "danger" => Tone::Danger,
                            _ => Tone::Info,
                        };
                        Confirm::new(ui.scope_id().with("confirm"), icon, "Confirm").tone(tone).content(ui, true, |_| {});
                    }
                }
            });
            let image = output
                .textures_delta
                .set
                .values()
                .flatten()
                .find_map(|delta| match &delta.image {
                    egui::ImageData::Color(image) if image.size == [side, side] => Some(image),
                    _ => None,
                })
                .expect("the surface must upload its icon texture");
            let colours: std::collections::HashSet<_> = image.pixels.iter().filter(|pixel| pixel.a() == 255).copied().collect();
            assert!(colours.len() >= 2, "{surface}/{icon}/{kind}/{tab}, dark={dark}: opaque artwork is a silhouette");
            let resolved = crate::icon_svg::resolve(icon);
            assert!(!resolved.placeholder, "{icon} needs a master");
            let look = crate::icon_svg::Look::themed(dark, ramp.and_then(crate::icon_svg::accent_index), solidcraft_theme::vision());
            let doc = crate::icon_svg::doc_for(resolved.master, side as u32).unwrap();
            let expected = if matches!(surface, "danger" | "danger_menu") {
                crate::icon_svg::render_tinted(doc, look, side as u32)
            } else {
                crate::icon_svg::render(doc, look, side as u32)
            }
            .unwrap();
            if icon == "delete" {
                assert!(colours.len() >= 3, "{surface}, dark={dark}: preserve the lid, faces and outline colours");
                let dominant = colours.iter().max_by_key(|colour| image.pixels.iter().filter(|pixel| *pixel == *colour).count()).unwrap();
                assert!(
                    dominant.r() > dominant.g() && dominant.r() > dominant.b(),
                    "{surface}, dark={dark}: dominant bin colour must be red, got {dominant:?}"
                );
            }
            assert_eq!(
                image.as_ref(),
                &egui::ColorImage::from_rgba_premultiplied([side, side], expected.data()),
                "{surface}/{kind}/{tab}: preserve roles with {ramp:?}"
            );
            if icon == "extrude" {
                let dominant = ["active-1", "active-2", "active-3"]
                    .map(|role| {
                        let colour = crate::icon_svg::role_colour(role, look);
                        (image.pixels.iter().filter(|pixel| **pixel == colour).count(), colour)
                    })
                    .into_iter()
                    .max_by_key(|(count, _)| *count)
                    .unwrap();
                assert!(dominant.0 > 0, "{surface}/{kind}/{tab}: missing accent pixels");
                let group = if matches!(surface, "warning" | "danger" | "info") { "confirm" } else { surface };
                dominant_accents.entry(group).or_default().insert(dominant.1);
            }
            if let Ok(dir) = std::env::var("SOLIDCRAFT_ICON_SURFACE_EVIDENCE") {
                let mut textures = std::collections::HashMap::new();
                crate::ui_kit::test_textures_update(&mut textures, &output.textures_delta);
                let capture = crate::ui_kit::test_raster(&ctx, output.shapes.clone(), output.pixels_per_point, bounds, &textures);
                std::fs::create_dir_all(&dir).unwrap();
                capture
                    .save(std::path::Path::new(&dir).join(format!("{surface}-{icon}-{kind}-{tab}-{}.png", if dark { "dark" } else { "light" })))
                    .unwrap();
            }
            output.textures_delta.clear();
        }
        for (group, count) in [("confirm", 3), ("radial", 5), ("radial_hover", 5), ("history_names", 5), ("history_icons", 5)] {
            assert_eq!(dominant_accents[group].len(), count, "{group}, dark={dark}: dominant accent pixels must differ across mappings");
        }
    }
    solidcraft_theme::set_dark(previous);
}

/// Run the application UI, including the ribbon, and optionally save each transition.
#[test]
fn command_search_ribbon_transition_clips_content_in_both_themes() {
    use crate::ui_kit::{test_raster as raster, test_textures_update as textures_update};
    use std::collections::HashMap;
    let previous = solidcraft_theme::is_dark();
    for dark in [false, true] {
        solidcraft_theme::set_dark(dark);
        let ctx = crate::fonts::test_context();
        ctx.set_visuals(if dark { egui::Visuals::dark() } else { egui::Visuals::light() });
        let mut app = crate::test_support::sample_app();
        app.home.open = false;
        app.ui.dark = dark;
        let bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(1600.0, 1000.0));
        let mut textures = HashMap::new();
        let mut closed = egui::Rect::NOTHING;
        for frame in 0..26 {
            if frame == 3 {
                crate::shortcut_box::open(&mut app, None);
                app.sbox.text = "fillet".into();
            } else if frame == 14 {
                app.ui.palette_open = false;
                app.sbox.text.clear();
            }
            solidcraft_ui_registry::clear_handles();
            let mut output = ctx.run_ui(
                egui::RawInput { screen_rect: Some(bounds), time: Some(f64::from(frame) / 60.0), focused: true, ..Default::default() },
                |ui| {
                    app.logic(ui.ctx());
                    app.ui(ui);
                },
            );
            textures_update(&mut textures, &output.textures_delta);
            output.textures_delta.clear();
            let field = solidcraft_ui_registry::rect_of("ribbon:search").unwrap();
            if frame == 2 {
                closed = field;
            }
            if (3..14).contains(&frame) {
                let sheet = solidcraft_ui_registry::rect_of("ribbon:search_popup").unwrap();
                assert!(sheet.top() > field.bottom() && sheet.top() <= field.bottom() + Tokens::get().space_6);
                // Actual result text is painted with the unfolding sheet's clip,
                // rather than relying on a smaller background hiding overflow.
                let mut result_texts = 0;
                for shape in &output.shapes {
                    if let egui::Shape::Text(text) = &shape.shape
                        && text.galley.job.text.contains("Fillet")
                        && text.pos.y >= sheet.top()
                        && text.pos.x >= sheet.left()
                        && text.pos.x < sheet.right()
                    {
                        result_texts += 1;
                        assert!(shape.clip_rect.bottom() <= sheet.bottom() + 0.5, "{frame}: {:?} vs {sheet:?}", shape.clip_rect);
                    }
                }
                if frame == 12 {
                    assert!(result_texts > 0);
                }
            }
            let state = match frame {
                2 => Some("closed"),
                4 => Some("opening"),
                12 => Some("open"),
                15 => Some("closing"),
                25 => Some("closed-again"),
                _ => None,
            };
            if frame == 12 {
                assert_eq!(field.size(), vec2(520.0, 40.0));
                assert_eq!(field.right(), closed.right());
            } else if frame == 25 {
                assert_eq!(field, closed);
            }
            if let (Some(state), Ok(dir)) = (state, std::env::var("SOLIDCRAFT_SEARCH_EVIDENCE")) {
                std::fs::create_dir_all(&dir).unwrap();
                let image = raster(&ctx, output.shapes, output.pixels_per_point, bounds, &textures);
                image.save(std::path::Path::new(&dir).join(format!("ribbon-{state}-{}.png", if dark { "dark" } else { "light" }))).unwrap();
            }
        }
    }
    solidcraft_theme::set_dark(previous);
}

/// Opt-in CPU capture of the actual File view, with public temporary fixtures only.
#[test]
#[ignore = "set SOLIDCRAFT_FILE_EVIDENCE to an output directory and run explicitly"]
fn file_view_application_evidence_in_both_themes_and_widths() {
    use crate::ui_kit::{test_raster as raster, test_textures_update as textures_update};
    use std::collections::HashMap;
    let output_dir = std::path::PathBuf::from(std::env::var("SOLIDCRAFT_FILE_EVIDENCE").expect("set SOLIDCRAFT_FILE_EVIDENCE"));
    std::fs::create_dir_all(&output_dir).unwrap();
    let fixture_dir = std::env::temp_dir().join(format!("solidcraft-file-evidence-{}", std::process::id()));
    std::fs::create_dir_all(&fixture_dir).unwrap();
    let path = fixture_dir.join("sample-plate.sc");
    let fixture = path.to_string_lossy().to_string();
    let mut sample = crate::test_support::sample_app();
    sample.session.execute("file.save_as", &serde_json::json!({"path":fixture})).unwrap();
    let previous = solidcraft_theme::is_dark();
    for dark in [false, true] {
        for (name, size) in [("desktop", vec2(1440.0, 1000.0)), ("narrow", vec2(420.0, 1000.0))] {
            let ctx = crate::fonts::test_context();
            let mut app = crate::test_support::sample_app();
            app.home.show(0);
            app.home.add_recent(&fixture);
            app.session.path = Some(fixture.clone());
            let preview = image::load_from_memory(include_bytes!("../../../assets/samples/exports/0.png")).unwrap().to_rgba8();
            let preview = egui::ColorImage::from_rgba_unmultiplied([preview.width() as usize, preview.height() as usize], preview.as_raw());
            crate::home::install(&mut app.home, &ctx, &fixture, Some(preview));
            app.preferences.theme = if dark { "dark" } else { "light" }.into();
            app.ui.dark = dark;
            app.recoverable.push(solidcraft_engine::recovery::Entry {
                id: "public-evidence".into(),
                path: Some("enclosure-lid.sc".into()),
                name: "Enclosure lid".into(),
                saved_at: web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).unwrap().as_secs().saturating_sub(120),
                features: 14,
            });
            solidcraft_theme::set_dark(dark);
            solidcraft_theme::apply(&ctx);
            let bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
            let mut textures = HashMap::new();
            for frame in 0..6 {
                let mut input =
                    egui::RawInput { screen_rect: Some(bounds), time: Some(f64::from(frame) / 60.0), focused: true, ..Default::default() };
                app.raw_input_hook(&mut input);
                let mut output = ctx.run_ui(input, |ui| {
                    app.logic(ui.ctx());
                    app.ui(ui);
                });
                textures_update(&mut textures, &output.textures_delta);
                if frame == 5 {
                    assert!(app.home.open);
                    let page = solidcraft_ui_registry::rect_of("home:page").expect("actual File view allocation");
                    assert!(bounds.contains_rect(page));
                    raster(&ctx, output.shapes.clone(), output.pixels_per_point, bounds, &textures)
                        .save(output_dir.join(format!("file-{name}-{}.png", if dark { "dark" } else { "light" })))
                        .unwrap();
                }
                output.textures_delta.clear();
            }
        }
    }
    solidcraft_theme::set_dark(previous);
    std::fs::remove_file(path).unwrap();
    std::fs::remove_dir(fixture_dir).unwrap();
}
