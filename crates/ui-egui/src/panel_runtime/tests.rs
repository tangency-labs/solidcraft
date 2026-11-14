#![cfg(test)]

use solidcraft_engine::panels::*;

use super::*;

fn surface_test_palette(dark: bool) -> solidcraft_theme::Tokens {
    let mut tokens = if dark { solidcraft_theme::Tokens::dark() } else { solidcraft_theme::Tokens::light() };
    tokens.panel = if dark { egui::Color32::BLACK } else { egui::Color32::WHITE };
    tokens.toolbar = tokens.panel;
    let ink = if dark { egui::Color32::WHITE } else { egui::Color32::BLACK };
    tokens.text = ink;
    tokens.text_dim = ink;
    tokens.form_ink = ink;
    tokens.form_dim = ink;
    tokens.dialog_label = ink;
    tokens
}

#[test]
fn panel_surface_policy_keeps_disabled_host_storage_small() {
    assert!(
        std::mem::size_of::<HostSurfaceStyle>() < std::mem::size_of::<solidcraft_theme::Tokens>(),
        "disabled host policy must not embed whole palettes in SolveApp"
    );
}

#[test]
fn panel_surface_contrast_covers_actual_light_dark_and_bundled_high_contrast() {
    crate::test_support::host_data();
    use egui::Color32;
    use solidcraft_color::srgb::{decode8, encode8};
    let high_contrast = crate::theme_files::resolve(crate::theme_files::find("high-contrast", &[]).unwrap());
    // Independent stored-premultiplied source-over for both real renderer target
    // formats. This does not call the production contrast/bounds predicate.
    let blend = |source: Color32, ground: Color32, linear: bool| {
        let remain = 1.0 - f64::from(source.a()) / 255.0;
        let channel = |source: u8, ground: u8| {
            if linear {
                encode8(decode8(source) + decode8(ground) * remain)
            } else {
                (f64::from(source) + f64::from(ground) * remain).round().clamp(0.0, 255.0) as u8
            }
        };
        Color32::from_rgb(channel(source.r(), ground.r()), channel(source.g(), ground.g()), channel(source.b(), ground.b()))
    };
    let inks = |tokens: &solidcraft_theme::Tokens| [tokens.text, tokens.text_dim, tokens.form_ink, tokens.form_dim, tokens.dialog_label];
    for (name, authored) in
        [("light", solidcraft_theme::Tokens::light()), ("dark", solidcraft_theme::Tokens::dark()), ("high-contrast", high_contrast)]
    {
        let ctx = egui::Context::default();
        let _scope = solidcraft_theme::scoped_panel_tokens(authored);
        assert!(inks(&authored).iter().all(|ink| ink.a() == 255), "these shipped canonical ink pairs are opaque: {name}");
        for floating in [false, true] {
            let raw = crate::ui_kit::panel_host_frame(&ctx, floating).fill.to_srgba_unmultiplied();
            let opaque = Color32::from_rgb(raw[0], raw[1], raw[2]);
            let impossible = inks(&authored).iter().any(|ink| solidcraft_theme::contrast_ratio(*ink, opaque) < 4.5);
            for translucency_percent in [0.0_f32, 50.0, 100.0] {
                let requested = 1.0 - translucency_percent / 100.0;
                let style = HostSurfaceStyle::capture(&ctx, false, Some(requested), None);
                let appearance = style.appearance(PanelSurface::Translucent, floating);
                let palette = style.palette(appearance);
                match appearance.contrast.as_ref().unwrap() {
                    ContrastOutcome::Readable { effective_background_alpha } => {
                        assert!(!impossible, "a shipped unreadable authored pair cannot be reported compliant: {name}");
                        assert!(*effective_background_alpha >= requested);
                        assert_eq!(inks(palette), inks(&authored), "ordinary resolution preserves every canonical ink: {name}");
                    }
                    ContrastOutcome::RetainReadableFallback { reason } => {
                        assert!(impossible, "fallback is justified by an independently checked opaque pair: {name}");
                        assert_eq!(*reason, ContrastFailure::UnreadableAtFullOpacity);
                        assert_eq!(appearance.fill.a(), 255);
                    }
                }
                for viewport in [0, 1, 64, 117, 192, 254, 255] {
                    for linear in [false, true] {
                        let background = blend(appearance.fill, Color32::from_gray(viewport), linear);
                        for ink in inks(palette) {
                            let physical_ink = blend(ink, background, linear);
                            assert!(
                                solidcraft_theme::contrast_ratio(physical_ink, background) >= 4.5,
                                "{name}: primary/muted text at translucency {translucency_percent}, viewport {viewport}, linear={linear}"
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn captured_panel_surfaces_protect_all_inks_at_requested_translucencies() {
    let ctx = egui::Context::default();
    for dark in [false, true] {
        let tokens = surface_test_palette(dark);
        let _scope = solidcraft_theme::scoped_panel_tokens(tokens);
        for translucency_percent in [0.0_f32, 50.0, 100.0] {
            let opacity = 1.0 - translucency_percent / 100.0;
            let style = HostSurfaceStyle::capture(&ctx, false, Some(opacity), None);
            for floating in [false, true] {
                let appearance = style.appearance(PanelSurface::Translucent, floating);
                let Some(ContrastOutcome::Readable { effective_background_alpha }) = appearance.contrast else {
                    panic!("valid theme must remain authored")
                };
                assert!(effective_background_alpha >= opacity);
                assert_eq!(effective_background_alpha, f32::from(appearance.fill.a()) / 255.0);
                assert_eq!(style.palette(appearance), &tokens);
                assert!(solidcraft_theme::panel_surface_readable(appearance.fill, style.palette(appearance)));
                if appearance.fill.a() > (opacity * 255.0).ceil() as u8 {
                    let [r, g, b, alpha] = appearance.fill.to_srgba_unmultiplied();
                    let previous = egui::Color32::from_rgba_unmultiplied(r, g, b, alpha - 1);
                    assert!(!solidcraft_theme::panel_surface_readable(previous, &tokens), "chosen byte is the first readable candidate");
                }
            }
            assert_eq!(style.appearance(PanelSurface::Opaque, false).fill.a(), 255);
        }
    }
}

#[test]
fn panel_contrast_checks_interior_luminance_and_translucent_muted_inks() {
    let mut tokens = surface_test_palette(false);
    // This ink has >=4.5 contrast against both black and white, yet an interior
    // viewport background can match it exactly. Endpoint checks alone are false.
    let middle = egui::Color32::from_gray(117);
    tokens.form_dim = middle;
    assert!(solidcraft_theme::contrast_ratio(middle, egui::Color32::BLACK) >= 4.5);
    assert!(solidcraft_theme::contrast_ratio(middle, egui::Color32::WHITE) >= 4.5);
    assert!(!solidcraft_theme::panel_surface_readable(egui::Color32::TRANSPARENT, &tokens));
    tokens.form_dim = egui::Color32::from_rgba_unmultiplied(0, 0, 0, 240);
    assert!(solidcraft_theme::panel_surface_readable(egui::Color32::WHITE, &tokens), "RGBA ink is composited rather than rejected");
    tokens.form_dim = egui::Color32::TRANSPARENT;
    assert!(!solidcraft_theme::panel_surface_readable(egui::Color32::WHITE, &tokens), "invisible muted ink cannot satisfy contrast");
}

#[test]
fn panel_contrast_encloses_actual_premultiplied_gpu_blends() {
    use egui::Color32;
    use solidcraft_color::srgb::{decode8, encode8};
    // Independent physical source-over oracle: read stored premultiplied bytes,
    // never the production readability predicate or unpremultiplication helper.
    let physical = |source: Color32, background: Color32, linear: bool, dither: f64| {
        let remain = 1.0 - f64::from(source.a()) / 255.0;
        let channel = |source: u8, background: u8| {
            let source = f64::from(source) + dither;
            if linear {
                // Test source channels here are integer and undithered on the
                // linear path; transfer-function fixtures independently cover it.
                encode8(decode8(source.round().clamp(0.0, 255.0) as u8) + decode8(background) * remain)
            } else {
                (source + f64::from(background) * remain).round().clamp(0.0, 255.0) as u8
            }
        };
        Color32::from_rgb(channel(source.r(), background.r()), channel(source.g(), background.g()), channel(source.b(), background.b()))
    };
    let premultiplied = Color32::from_rgba_premultiplied(32, 32, 32, 128);
    let ground = Color32::from_gray(1);
    assert_eq!(physical(premultiplied, ground, false, 0.0), Color32::from_gray(32));
    assert_eq!(solidcraft_theme::composite(premultiplied, ground), Color32::from_gray(33), "round-trip reconstruction is a different blend");
    for background in [0, 1, 64, 117, 192, 254, 255] {
        let ground = Color32::from_gray(background);
        let (low, high) = solidcraft_theme::panel_composite_bounds(premultiplied, ground);
        for (linear, dither) in [(false, -0.475), (false, 0.0), (false, 0.475), (true, 0.0)] {
            let actual = physical(premultiplied, ground, linear, dither);
            assert!(low.r() <= actual.r() && actual.r() <= high.r(), "actual stored-byte blend lies inside conservative bounds");
        }
    }
    let ctx = egui::Context::default();
    for dark in [false, true] {
        let mut tokens = surface_test_palette(dark);
        tokens.form_dim = if dark { Color32::from_rgba_unmultiplied(255, 255, 255, 240) } else { Color32::from_rgba_unmultiplied(0, 0, 0, 240) };
        let _scope = solidcraft_theme::scoped_panel_tokens(tokens);
        let style = HostSurfaceStyle::capture(&ctx, false, Some(0.0), None);
        let appearance = style.appearance(PanelSurface::Translucent, false);
        assert!(matches!(appearance.contrast, Some(ContrastOutcome::Readable { .. })));
        for viewport in [0, 1, 64, 117, 192, 254, 255] {
            for linear in [false, true] {
                let background = physical(appearance.fill, Color32::from_gray(viewport), linear, 0.0);
                for ink in [tokens.text, tokens.text_dim, tokens.form_ink, tokens.form_dim, tokens.dialog_label] {
                    let text = physical(ink, background, linear, 0.0);
                    assert!(
                        solidcraft_theme::contrast_ratio(text, background) >= 4.5,
                        "independent framebuffer blend preserves both primary and muted text"
                    );
                }
            }
        }
    }
}

#[test]
fn panel_surface_cache_tracks_dark_mode_even_with_identical_authored_tokens() {
    struct RestoreDark(bool);
    impl Drop for RestoreDark {
        fn drop(&mut self) {
            solidcraft_theme::set_dark(self.0);
        }
    }
    let _restore = RestoreDark(solidcraft_theme::is_dark());
    let ctx = egui::Context::default();
    let mut tokens = surface_test_palette(false);
    tokens.form_dim = tokens.panel;
    let _scope = solidcraft_theme::scoped_panel_tokens(tokens);
    solidcraft_theme::set_dark(false);
    let light = HostSurfaceStyle::capture(&ctx, false, Some(0.5), None);
    solidcraft_theme::set_dark(true);
    let dark = HostSurfaceStyle::capture(&ctx, false, Some(0.5), Some(&light));
    let light_appearance = light.appearance(PanelSurface::Translucent, false);
    let dark_appearance = dark.appearance(PanelSurface::Translucent, false);
    assert_ne!(light_appearance.fill, dark_appearance.fill, "fallback family changes despite identical captured authored tokens");
    assert_eq!(HostSurfaceStyle::capture(&ctx, false, Some(0.5), Some(&dark)), dark);
}

#[test]
fn invalid_panel_theme_reports_failure_and_uses_opaque_known_tokens() {
    let ctx = egui::Context::default();
    let mut tokens = surface_test_palette(false);
    tokens.form_dim = tokens.panel;
    tokens.space_4 = 7.0;
    let _scope = solidcraft_theme::scoped_panel_tokens(tokens);
    for override_alpha in [Some(0.5), Some(f32::NAN), Some(f32::INFINITY), Some(-0.1), Some(1.1)] {
        let style = HostSurfaceStyle::capture(&ctx, false, override_alpha, None);
        let appearance = style.appearance(PanelSurface::Translucent, false);
        let expected = if override_alpha.is_some_and(|a| !a.is_finite() || !(0.0..=1.0).contains(&a)) {
            ContrastFailure::InvalidTokens
        } else {
            ContrastFailure::UnreadableAtFullOpacity
        };
        assert_eq!(appearance.contrast, Some(ContrastOutcome::RetainReadableFallback { reason: expected }));
        assert_eq!(appearance.fill.a(), 255);
        assert!(solidcraft_theme::panel_surface_readable(appearance.fill, style.palette(appearance)));
        assert_eq!(style.palette(appearance).space_4, tokens.space_4, "fallback does not change layout metrics");
        let cached = HostSurfaceStyle::capture(&ctx, false, override_alpha, Some(&style));
        assert_eq!(cached, style, "even an invalid raw NaN override has a stable bitwise cache key");
    }
}

#[test]
fn panel_surface_capture_matches_shared_frames_and_respects_accessibility() {
    let ctx = egui::Context::default();
    let mut tokens = surface_test_palette(false);
    tokens.toolbar = egui::Color32::from_rgba_unmultiplied(220, 230, 240, 180);
    let _scope = solidcraft_theme::scoped_panel_tokens(tokens);
    let style = HostSurfaceStyle::capture(&ctx, false, None, None);
    for floating in [false, true] {
        assert_eq!(style.appearance(PanelSurface::Translucent, floating).fill, tokens.toolbar);
    }
    assert_eq!(HostSurfaceStyle::capture(&ctx, false, None, Some(&style)), style);
    let forced = HostSurfaceStyle::capture(&ctx, true, Some(0.0), Some(&style));
    assert_ne!(forced, style);
    for floating in [false, true] {
        let appearance = forced.appearance(PanelSurface::Translucent, floating);
        assert_eq!(appearance.fill.a(), 255);
        assert!(solidcraft_theme::panel_surface_readable(appearance.fill, forced.palette(appearance)));
    }
    let second_context = egui::Context::default();
    assert_eq!(HostSurfaceStyle::capture(&second_context, false, None, Some(&style)), style);
    let mut changed = tokens;
    changed.text_dim = egui::Color32::from_gray(20);
    let _changed = solidcraft_theme::scoped_panel_tokens(changed);
    assert_ne!(HostSurfaceStyle::capture(&ctx, false, None, Some(&style)), style);
}

#[test]
fn panel_paint_palette_scope_restores_nested_tokens_without_changing_base_theme() {
    let before = solidcraft_theme::Tokens::get();
    let base = solidcraft_theme::Tokens::base();
    let mut outer = before;
    outer.text = outer.file_primary_ink;
    {
        let _outer = solidcraft_theme::scoped_panel_tokens(outer);
        assert_eq!(solidcraft_theme::Tokens::get(), outer);
        assert_eq!(solidcraft_theme::Tokens::get_in(solidcraft_theme::ColorVision::Off), outer);
        assert_eq!(solidcraft_theme::Tokens::base(), base);
        {
            let inner = surface_test_palette(false);
            let _inner = solidcraft_theme::scoped_panel_tokens(inner);
            assert_eq!(solidcraft_theme::Tokens::get(), inner);
        }
        assert_eq!(solidcraft_theme::Tokens::get(), outer);
    }
    assert_eq!(solidcraft_theme::Tokens::get(), before);
    assert_eq!(solidcraft_theme::Tokens::base(), base);
}

fn prepare_preference_fixture(app: &mut crate::SolveApp) {
    app.panel_runtime
        .as_mut()
        .unwrap()
        .prepare_host(&app.session, LogicalRect { origin: Point { x: 0.0, y: 0.0 }, size: Size { width: 1600.0, height: 1000.0 } })
        .unwrap();
}

fn remembered_preference_fixture() -> crate::SolveApp {
    let mut app = crate::SolveApp::new(solidcraft_engine::Session::default(), Default::default());
    super::specimen::control_specimen(&mut app, &serde_json::json!({"preset":"docks","remember":true})).unwrap();
    prepare_preference_fixture(&mut app);
    app
}

#[test]
fn application_panel_preferences_round_trip_named_layouts_columns_and_expansion() {
    let mut app = remembered_preference_fixture();
    assert!(app.panel_runtime().unwrap().layout().inspect().capabilities.persistence);
    let document = app.session.doc.clone();
    let revision = app.session.revision;
    let comparison = panel_layout_snapshot(&app);
    assert!(panel_layout_matches(&app, &comparison));
    let group = app.panel_runtime().unwrap().layout().layout().sections.first().unwrap().id.clone();
    app.dispatch_panel_layout(LayoutAction::Apply {
        expected_revision: None,
        mutations: vec![Mutation::Collapse { section: group, collapsed: true }],
    })
    .unwrap();
    assert!(!panel_layout_matches(&app, &comparison), "Apply observes a committed layout change without serializing the live catalogue");
    app.run_panel_command("layout.save", &serde_json::json!({"name":"My arrangement"})).unwrap();
    let layout = app.panel_runtime().unwrap().layout().layout().clone();
    let saved = app.prefs();
    assert!(serde_json::from_str::<serde_json::Value>(&saved).unwrap()[PREFERENCES_KEY]["catalogue"].is_string());
    assert!(std::sync::Arc::ptr_eq(&document, &app.session.doc));
    assert_eq!(revision, app.session.revision);
    let mut restored = crate::SolveApp::new(solidcraft_engine::Session::default(), Default::default());
    restored.load_prefs(&saved);
    assert!(restored.panel_runtime().is_none(), "saved layouts do not enable a host or alter Classic defaults");
    super::specimen::control_specimen(&mut restored, &serde_json::json!({"preset":"docks","remember":true})).unwrap();
    assert!(restored.panel_runtime.as_ref().unwrap().preferences.as_ref().unwrap().pending.is_some());
    prepare_preference_fixture(&mut restored);
    assert_eq!(restored.panel_runtime().unwrap().layout().layout(), &layout);
    assert_eq!(user_preset_names(&restored), ["My arrangement"]);
    restored.run_panel_command("layout.reset", &serde_json::json!({})).unwrap();
    assert_eq!(user_preset_names(&restored), ["My arrangement"], "workspace reset retains the named library");
}

#[test]
fn application_panel_workspace_selection_is_explicit_and_remembered() {
    let mut app = remembered_preference_fixture();
    let document = app.session.doc.clone();
    let selected = app.panel_runtime().unwrap().layout().layout().workspace.clone();
    app.ui.tab = "Render".into();
    app.tool = crate::tools::Tool::for_command("sketch.line3d");
    app.session.document_id = "other-document".into();
    app.tick_panel_runtime().unwrap();
    assert_eq!(app.panel_runtime().unwrap().layout().layout().workspace, selected);
    app.run_panel_command("layout.workspace.select", &serde_json::json!({"workspace":"panel_fixture.workspace.review"})).unwrap();
    assert_eq!(app.ui.tab, "Render");
    assert_eq!(app.tool.as_ref().map(|tool| tool.cmd), Some("sketch.line3d"));
    assert_eq!(app.session.document_id, "other-document");
    assert!(std::sync::Arc::ptr_eq(&document, &app.session.doc));
    let saved = app.prefs();
    let mut restored = crate::SolveApp::new(solidcraft_engine::Session::default(), Default::default());
    restored.load_prefs(&saved);
    super::specimen::control_specimen(&mut restored, &serde_json::json!({"preset":"docks","remember":true})).unwrap();
    prepare_preference_fixture(&mut restored);
    assert_eq!(restored.panel_runtime().unwrap().layout().layout().workspace.0, "panel_fixture.workspace.review");
}

#[test]
fn future_application_panel_record_is_retained_without_legacy_reimport() {
    let future = serde_json::json!({"version":99,"future":{"exact":[1,"two",null]}});
    let mut app = crate::SolveApp::new(solidcraft_engine::Session::default(), Default::default());
    app.load_prefs(&serde_json::json!({"panel_layout":future,"preferences":{"browser_width":777.0}}).to_string());
    assert_eq!(preferences_value(&app), Some(future.clone()));
    super::specimen::control_specimen(&mut app, &serde_json::json!({"preset":"docks","remember":true})).unwrap();
    assert_eq!(app.status.as_ref().map(|status| status.0.as_str()), Some(app.tr("prefs.panel_layout.recovery")));
    prepare_preference_fixture(&mut app);
    let value = preferences_value(&app).unwrap();
    assert_eq!(value.get("rejected"), Some(&future));
    assert!(value.get("legacy").is_some_and(serde_json::Value::is_null), "a present unreadable record blocks old-field migration");
    let saved = app.prefs();
    let mut restored = crate::SolveApp::new(solidcraft_engine::Session::default(), Default::default());
    restored.load_prefs(&saved);
    super::specimen::control_specimen(&mut restored, &serde_json::json!({"preset":"docks","remember":true})).unwrap();
    prepare_preference_fixture(&mut restored);
    assert_eq!(preferences_value(&restored).unwrap().get("rejected"), Some(&future), "recovery does not nest on every restart");
}

#[test]
fn invalid_packed_catalogue_and_unknown_legacy_fields_remain_recoverable() {
    let mut app = crate::SolveApp::new(solidcraft_engine::Session::default(), Default::default());
    app.load_prefs("{\"preferences\":{\"browser_width\":333}}");
    super::specimen::control_specimen(&mut app, &serde_json::json!({"preset":"docks","remember":true})).unwrap();
    let valid = preferences_value(&app).unwrap();
    let mut unknown_legacy = valid.clone();
    unknown_legacy["legacy"]["future_placement"] = "keep this".into();
    for malformed in ["!invalid!", "Zg", "Zh=="] {
        let mut raw = valid.clone();
        raw["catalogue"] = malformed.into();
        let mut restored = crate::SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        restored.load_prefs(&serde_json::json!({"panel_layout":raw}).to_string());
        super::specimen::control_specimen(&mut restored, &serde_json::json!({"preset":"docks","remember":true})).unwrap();
        assert_eq!(preferences_value(&restored).unwrap()["rejected"], raw);
    }
    let mut restored = crate::SolveApp::new(solidcraft_engine::Session::default(), Default::default());
    restored.load_prefs(&serde_json::json!({"panel_layout":unknown_legacy}).to_string());
    super::specimen::control_specimen(&mut restored, &serde_json::json!({"preset":"docks","remember":true})).unwrap();
    assert_eq!(preferences_value(&restored).unwrap()["rejected"], unknown_legacy);
}

#[test]
fn legacy_panel_placement_inputs_survive_unmapped_fixture_restart() {
    let mut app = crate::SolveApp::new(solidcraft_engine::Session::default(), Default::default());
    app.load_prefs(&serde_json::json!({"preferences":{"browser_width":333.0,"agent_chat_popped":true}}).to_string());
    super::specimen::control_specimen(&mut app, &serde_json::json!({"preset":"docks","remember":true})).unwrap();
    prepare_preference_fixture(&mut app);
    let first = preferences_value(&app).unwrap();
    assert_eq!(first["legacy"]["browser_width"], 333.0);
    assert_eq!(first["legacy"]["imported"], serde_json::json!([]));
    let saved = app.prefs();
    let mut restored = crate::SolveApp::new(solidcraft_engine::Session::default(), Default::default());
    restored.load_prefs(&saved);
    super::specimen::control_specimen(&mut restored, &serde_json::json!({"preset":"docks","remember":true})).unwrap();
    prepare_preference_fixture(&mut restored);
    assert_eq!(preferences_value(&restored).unwrap()["legacy"], first["legacy"]);
    migrate_legacy_placements(
        &mut restored,
        &[LegacyPanelBinding { role: LegacyPanelRole::Browser, instance: InstanceId::new("specimen-0").unwrap() }],
    )
    .unwrap();
    assert_eq!(preferences_value(&restored).unwrap()["legacy"]["imported"], serde_json::json!(["browser"]));
}

#[test]
fn legacy_panel_capture_uses_the_presented_collapse_and_history_location() {
    for (left, under, card_collapsed, column_collapsed, collapsed, stacked) in
        [(true, false, false, true, true, false), (true, true, true, false, false, true), (false, true, true, false, true, false)]
    {
        let mut app = crate::SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        app.load_prefs(
            &serde_json::json!({"preferences":{
                "timeline_left":left,"timeline_under_browser":under,
                "browser_collapsed":card_collapsed,"timeline_left_browser_collapsed":column_collapsed,
            }})
            .to_string(),
        );
        super::specimen::control_specimen(&mut app, &serde_json::json!({"preset":"docks","remember":true})).unwrap();
        let saved = preferences_value(&app).unwrap();
        assert_eq!(saved["legacy"]["browser_collapsed"], collapsed);
        assert_eq!(saved["legacy"]["history_left"], left);
        assert_eq!(saved["legacy"]["history_under_browser"], stacked);
    }
}

#[test]
fn panel_profiles_keep_current_placement_without_named_library_or_recovery() {
    let mut app = remembered_preference_fixture();
    app.run_panel_command("layout.save", &serde_json::json!({"name":"Not in profile"})).unwrap();
    let profile = profile_value(&app).unwrap();
    validate_profile_value(&profile).unwrap();
    assert!(!profile.to_string().contains("Not in profile"));
    let before = app.panel_runtime().unwrap().layout().layout().clone();
    let group = before.sections.first().unwrap().id.clone();
    app.dispatch_panel_layout(LayoutAction::Apply {
        expected_revision: None,
        mutations: vec![Mutation::Collapse { section: group, collapsed: true }],
    })
    .unwrap();
    apply_profile_value(&mut app, &profile).unwrap();
    assert_eq!(app.panel_runtime().unwrap().layout().layout(), &before);
    assert_eq!(user_preset_names(&app), ["Not in profile"]);
    assert!(validate_profile_value(&serde_json::json!({"version":1,"envelope":"x".repeat(256 * 1024 + 1)})).is_err());
    let mut incoming: LayoutEnvelope = serde_json::from_str(profile["workspaces"][before.workspace.0.as_str()].as_str().unwrap()).unwrap();
    incoming.layout.instances.first_mut().unwrap().kind = PanelKind::new("unavailable.extension").unwrap();
    let unavailable = serde_json::json!({"version":1,"envelope":serde_json::to_string(&incoming).unwrap()});
    validate_profile_value(&unavailable).unwrap();
    let saved = preferences_value(&app);
    assert_eq!(apply_profile_value(&mut app, &unavailable).unwrap_err().rule, Rule::Availability);
    assert_eq!(preferences_value(&app), saved, "a missing extension must not silently disappear from the imported layout");
}

#[test]
fn panel_profiles_restore_each_workspace_and_selection_without_changing_the_machine_library() {
    let mut app = remembered_preference_fixture();
    let first = app.panel_runtime().unwrap().layout().layout().workspace.clone();
    let section = app.panel_runtime().unwrap().layout().layout().sections.first().unwrap().id.clone();
    app.dispatch_panel_layout(transaction(vec![Mutation::Collapse { section, collapsed: true }])).unwrap();
    let first_arrangement = app.panel_runtime().unwrap().layout().layout().clone();
    app.run_panel_command("layout.workspace.select", &serde_json::json!({"workspace":"panel_fixture.workspace.review"})).unwrap();
    prepare_preference_fixture(&mut app);
    let selected_arrangement = app.panel_runtime().unwrap().layout().layout().clone();
    let profile = profile_value(&app).unwrap();
    assert_eq!(profile["version"], 2);
    assert_eq!(profile["selected"], "panel_fixture.workspace.review");
    assert_eq!(profile["workspaces"].as_object().unwrap().len(), 5, "unvisited workspaces contribute their defaults too");
    assert_eq!(profile.as_object().unwrap().len(), 3, "no named library is copied into the profile");

    app.run_panel_command("layout.save", &serde_json::json!({"name":"Machine-only layout"})).unwrap();
    app.run_panel_command("layout.reset", &serde_json::json!({})).unwrap();
    app.run_panel_command("layout.workspace.select", &serde_json::json!({"workspace":first})).unwrap();
    prepare_preference_fixture(&mut app);
    app.run_panel_command("layout.reset", &serde_json::json!({})).unwrap();
    apply_profile_value(&mut app, &profile).unwrap();
    assert_eq!(app.panel_runtime().unwrap().layout().layout(), &selected_arrangement);
    assert_eq!(user_preset_names(&app), ["Machine-only layout"]);
    app.run_panel_command("layout.workspace.select", &serde_json::json!({"workspace":first})).unwrap();
    assert_eq!(app.panel_runtime().unwrap().layout().layout(), &first_arrangement, "inactive arrangements were restored atomically too");
}

#[test]
fn panel_layout_tile_selection_tracks_complete_arrangements_and_manual_reversion() {
    let mut app = remembered_preference_fixture();
    let initial = layout_preferences_view(&app).unwrap();
    assert!(initial.default_matches);
    assert_eq!(initial.entries.iter().filter(|entry| entry.selected).count(), 1);
    let section = initial.current_layout.sections.first().unwrap().id.clone();
    app.dispatch_panel_layout(transaction(vec![Mutation::Collapse { section: section.clone(), collapsed: true }])).unwrap();
    let changed = layout_preferences_view(&app).unwrap();
    assert!(!changed.default_matches);
    assert!(!changed.entries.iter().any(|entry| entry.selected), "manual changes show Custom instead of a stale selected tile");
    app.dispatch_panel_layout(transaction(vec![Mutation::Collapse { section, collapsed: false }])).unwrap();
    let reverted = layout_preferences_view(&app).unwrap();
    assert!(reverted.default_matches);
    assert_eq!(reverted.entries.iter().filter(|entry| entry.selected).count(), 1);
}

#[test]
fn panel_preference_disk_projection_removes_native_geometry_without_changing_the_live_or_recovery_state() {
    use commands::LayoutCatalogue as _;
    let mut fixture = PanelMountHarness::new(true).unwrap();
    fixture.command("panel.open", &panel_command_editor()).unwrap();
    fixture.command("panel.popout", &panel_command_native_move()).unwrap();
    fixture.command("layout.save", &serde_json::json!({"name":"Native arrangement"})).unwrap();
    let before = fixture.runtime.layout().layout().clone();
    assert!(before.sections.iter().any(|section| section.location.zone() == Zone::Window));
    let rejected = serde_json::json!({"future_recovery":[1,"keep",null]});
    let record = super::preferences::PreferenceRecord {
        version: 1,
        selected: before.workspace.clone(),
        catalogue: fixture.catalogue.state().export().unwrap(),
        rejected: Some(rejected.clone()),
        legacy: None,
    }
    .value();
    let mut observer = crate::SolveApp::new(Default::default(), Default::default());
    let empty = PanelMountHarness::new(false).unwrap();
    observer
        .enable_panel_runtime(
            empty.runtime,
            Box::new(AppFixtureServices {
                models: FakeModels { editable: true },
                commands: FakeContentCommands::default(),
                local: FakeLocalServices::default(),
                refreshed: 0,
            }),
        )
        .unwrap();
    let portable = portable_preferences_value(&observer, Some(record.clone())).unwrap().unwrap();
    let registry = observer.panel_runtime().unwrap().layout().registry();
    let catalogue = super::preferences::PreferenceRecord::read(&portable, registry).unwrap().catalogue(registry).unwrap();
    catalogue
        .map_envelopes(registry, |_, envelope| {
            assert!(envelope.layout.sections.iter().all(|section| section.location.zone() != Zone::Window));
            assert!(envelope.layout.instances.iter().all(|instance| instance.memory.location.zone() != Zone::Window));
            Ok(envelope)
        })
        .unwrap();
    assert_eq!(portable["rejected"], rejected);
    assert_eq!(fixture.runtime.layout().layout(), &before, "disk projection does not redock the live native panel");
    assert_eq!(record["rejected"], rejected, "the exact rollback record remains untouched");

    fixture.dispatch(vec![fixture_open("fixture.log", "return-0", "full-return", Zone::Left, 0).unwrap()]).unwrap();
    let tabs = (1..MAX_TABS)
        .map(|index| Mutation::Open {
            kind: PanelKind::new("fixture.log").unwrap(),
            instance: id(&format!("return-{index}")),
            destination: Some(Destination::JoinTab { section: section("full-return"), index }),
        })
        .collect();
    fixture.dispatch(tabs).unwrap();
    let mut blocked = fixture.runtime.layout().layout().clone();
    let column = blocked.section(&section("full-return")).unwrap().column.clone();
    if let SectionLocation::Window { return_to, .. } =
        &mut blocked.sections.iter_mut().find(|section| section.location.zone() == Zone::Window).unwrap().location
    {
        return_to.column = column;
        return_to.join = Some(ReturnTab { section: section("full-return"), index: 0 });
    }
    validate(&blocked, registry).unwrap();
    fixture.catalogue.remember(&LayoutEnvelope::new(blocked.clone()), None, registry).unwrap();
    let blocked_record = super::preferences::PreferenceRecord {
        version: 1,
        selected: blocked.workspace.clone(),
        catalogue: fixture.catalogue.state().export().unwrap(),
        rejected: None,
        legacy: None,
    }
    .value();
    let fallback = portable_preferences_value(&observer, Some(blocked_record)).unwrap().unwrap();
    let state = super::preferences::PreferenceRecord::read(&fallback, registry).unwrap().catalogue(registry).unwrap();
    let envelope = state.remembered(&blocked.workspace).unwrap().envelope(registry, &blocked.workspace).unwrap();
    let returned = envelope.layout.containing(&id("editor")).unwrap();
    assert_eq!(
        returned.location,
        SectionLocation::Floating { origin: Point { x: 0.0, y: 0.0 } },
        "a full return group uses portable Floating defaults"
    );
    assert_eq!(returned.size, registry.get(&PanelKind::new("fixture.editor").unwrap()).unwrap().default_size);
}

#[test]
fn unchanged_pending_record_is_not_a_successful_restore_until_actual_admission_succeeds() {
    let mut source = crate::SolveApp::new(Default::default(), Default::default());
    super::specimen::control_specimen(&mut source, &serde_json::json!({"preset":"overflow","remember":true})).unwrap();
    let saved = source.prefs();
    let mut app = crate::SolveApp::new(Default::default(), Default::default());
    app.load_prefs(&saved);
    super::specimen::control_specimen(&mut app, &serde_json::json!({"preset":"docks","remember":true})).unwrap();
    let small = LogicalRect { origin: Point { x: 0.0, y: 0.0 }, size: Size { width: 300.0, height: 200.0 } };
    app.panel_runtime.as_mut().unwrap().prepare_host(&app.session, small).unwrap();
    let baseline = preferences_value(&app);
    let fallback = app.panel_runtime().unwrap().layout().layout().clone();
    assert!(app.panel_runtime.as_ref().unwrap().preferences.as_ref().unwrap().pending.is_some());
    assert!(restore_preferences(&mut app, baseline.clone()).is_err(), "record equality cannot claim the fallback is the restored layout");
    assert_eq!(preferences_value(&app), baseline);
    assert_eq!(app.panel_runtime().unwrap().layout().layout(), &fallback);
    assert!(app.panel_runtime.as_ref().unwrap().preferences.as_ref().unwrap().pending.is_some());
    let large = LogicalRect { origin: Point { x: 0.0, y: 0.0 }, size: Size { width: 6000.0, height: 6000.0 } };
    app.panel_runtime.as_mut().unwrap().prepare_host(&app.session, large).unwrap();
    assert!(
        app.panel_runtime.as_ref().unwrap().preferences.as_ref().unwrap().pending.is_none(),
        "capacity changes retry the actual saved arrangement"
    );
}

#[test]
fn preference_restore_retains_live_controller_draft_and_failed_admission_is_atomic() {
    let mut fixture = PanelMountHarness::new(false).unwrap();
    open_editor(&mut fixture);
    let probe = fixture.probe.clone();
    let mut app = crate::SolveApp::new(solidcraft_engine::Session::default(), Default::default());
    app.enable_panel_runtime(
        fixture.runtime,
        Box::new(AppFixtureServices {
            models: FakeModels { editable: true },
            commands: FakeContentCommands::default(),
            local: FakeLocalServices::default(),
            refreshed: 0,
        }),
    )
    .unwrap();
    app.enable_universal_panel_host(HostOptions::default()).unwrap();
    enable_preferences(&mut app).unwrap();
    prepare_preference_fixture(&mut app);
    let owner = ContentOwner::Document(DocumentId::new(app.session.document_id.clone()).unwrap());
    let key = ControllerKey { instance: id("editor"), owner };
    probe
        .edit(&key, |state| {
            state.draft = "37 mm retained draft".into();
            state.reject_cancel = true;
        })
        .unwrap();
    let saved = preferences_value(&app);
    let section = app.panel_runtime().unwrap().layout().layout().containing(&id("editor")).unwrap().id.clone();
    app.dispatch_panel_layout(transaction(vec![Mutation::Collapse { section, collapsed: true }])).unwrap();
    restore_preferences(&mut app, saved.clone()).unwrap();
    let controller_count = app.panel_runtime().unwrap().controller_count();
    assert_eq!(probe.state(&key).unwrap().draft, "37 mm retained draft");
    assert!(probe.state(&key).unwrap().transaction_live);
    assert_eq!(probe.state(&key).unwrap().cancels, 0);

    let mut larger = crate::SolveApp::new(solidcraft_engine::Session::default(), Default::default());
    super::specimen::control_specimen(&mut larger, &serde_json::json!({"preset":"overflow","remember":true})).unwrap();
    let before = preferences_value(&app);
    let layout = app.panel_runtime().unwrap().layout().inspect();
    let document = app.session.doc.clone();
    assert!(restore_preferences(&mut app, preferences_value(&larger)).is_err());
    assert_eq!(preferences_value(&app), before);
    assert_eq!(app.panel_runtime().unwrap().layout().inspect(), layout);
    assert_eq!(app.panel_runtime().unwrap().controller_count(), controller_count);
    assert!(std::sync::Arc::ptr_eq(&document, &app.session.doc));
    assert_eq!(probe.state(&key).unwrap().draft, "37 mm retained draft");
}

#[test]
fn unchanged_absent_record_cancel_preserves_deferred_legacy_and_explicit_null_recovery() {
    let mut app = crate::SolveApp::new(solidcraft_engine::Session::default(), Default::default());
    app.load_prefs(&serde_json::json!({"preferences":{"browser_width":333.0}}).to_string());
    restore_preferences(&mut app, None).unwrap();
    super::specimen::control_specimen(&mut app, &serde_json::json!({"preset":"docks","remember":true})).unwrap();
    assert_eq!(preferences_value(&app).unwrap()["legacy"]["browser_width"], 333.0);
    let mut broken = crate::SolveApp::new(solidcraft_engine::Session::default(), Default::default());
    broken.load_prefs("{\"panel_layout\":null}");
    super::specimen::control_specimen(&mut broken, &serde_json::json!({"preset":"docks","remember":true})).unwrap();
    let saved = broken.prefs();
    let mut restored = crate::SolveApp::new(solidcraft_engine::Session::default(), Default::default());
    restored.load_prefs(&saved);
    super::specimen::control_specimen(&mut restored, &serde_json::json!({"preset":"docks","remember":true})).unwrap();
    prepare_preference_fixture(&mut restored);
    assert!(preferences_value(&restored).unwrap().as_object().unwrap().contains_key("rejected"));
    assert_eq!(preferences_value(&restored).unwrap()["rejected"], serde_json::Value::Null);
}

/// Immutable analytic metrics fixture. These cases do not render bodies and do
/// not establish visual fidelity.
#[derive(Debug)]
struct AllocationMetricsFixture {
    revision: u64,
    policies: std::collections::BTreeMap<InstanceId, SideHeightPolicy>,
    width: Option<f32>,
    narrow_height: f32,
    wide_height: f32,
}
impl HostContentMetrics for AllocationMetricsFixture {
    fn revision(&self) -> u64 {
        self.revision
    }
    fn measure(&self, request: &ContentMeasureRequest) -> PanelResult<SideContentMeasure> {
        if self.width.is_some_and(|width| width != request.width) {
            return Err(error(Rule::Revision, "fixture has no metrics at requested width"));
        }
        Ok(SideContentMeasure {
            request: request.clone(),
            needed_height: (if request.width < 300.0 { self.narrow_height } else { self.wide_height }).max(request.chrome_height),
            policy: self.policies.get(&request.instance).copied().unwrap_or(SideHeightPolicy::Content),
        })
    }
}
fn fixture_complete_reservations(service: &LayoutService) -> HostIdentityReservations {
    // This fixture owns the complete synthetic envelope, including all memories.
    // Production bare defaults remain incomplete and cannot author priorities.
    host_identity_reservations(&LayoutEnvelope::new(service.layout().clone())).unwrap()
}
fn fixture_metric_policy(service: &LayoutService) -> HostGeometryPolicy {
    let policies = service
        .layout()
        .instances
        .iter()
        .map(|instance| (instance.id.clone(), service.registry().get(&instance.kind).unwrap().side_height_policy))
        .collect();
    HostGeometryPolicy {
        measure: Some(std::sync::Arc::new(AllocationMetricsFixture { revision: 7, policies, width: None, narrow_height: 240.0, wide_height: 240.0 })),
        content_revision: 7,
        identity_reservations: fixture_complete_reservations(service),
        ..HostGeometryPolicy::default()
    }
}
fn host_layout(h: &PanelMountHarness, width: f32, height: f32, direction: Direction) -> HostLayout {
    resolve_host_layout(
        h.runtime.layout(),
        LogicalRect { origin: Point { x: 0.0, y: 0.0 }, size: Size { width, height } },
        direction,
        HostMetrics::default(),
        &fixture_metric_policy(h.runtime.layout()),
    )
    .unwrap()
}
fn allocate_fixture(service: &LayoutService, size: Size, direction: Direction, policy: &HostGeometryPolicy) -> HostLayout {
    let mut policy = policy.clone();
    if policy.identity_reservations == HostIdentityReservations::default() {
        policy.identity_reservations = fixture_complete_reservations(service);
    }
    resolve_host_layout(
        service,
        LogicalRect { origin: Point { x: 0.0, y: 30.0 }, size },
        direction,
        HostMetrics {
            header_height: 24.0,
            footer_height: 16.0,
            inline_max_height: 88.0,
            content_header_height: 0.0,
            min_viewport: Size { width: 8.0, height: 8.0 },
            ..HostMetrics::default()
        },
        &policy,
    )
    .unwrap()
}
fn apply_fixture(service: &mut LayoutService, mutations: Vec<Mutation>) {
    let prepared = service.prepare(transaction(mutations)).unwrap();
    service.commit(prepared).unwrap();
}
fn fixture_scope(service: &LayoutService) -> LocalScope {
    LocalScope { workspace: service.layout().workspace.clone(), document: None, viewport: ViewportId::new("fixture.viewport").unwrap() }
}
fn fixture_press(service: &LayoutService, layout: &HostLayout, interaction: &mut HostInteraction, target: HostPointerTarget) -> Point {
    let at = middle(layout.control(&target).unwrap().bounds);
    let update = interaction.update(service, &fixture_scope(service), layout, HostPointerEvent::Press { at, target: Some(target) });
    assert!(update.rejection.is_none(), "{update:?}");
    assert!(update.candidate.is_none());
    at
}
fn fixture_move(service: &LayoutService, layout: &HostLayout, interaction: &mut HostInteraction, at: Point) -> HostPreview {
    let update = interaction.update(service, &fixture_scope(service), layout, HostPointerEvent::Move { at });
    assert!(update.action.is_none());
    assert!(update.candidate.is_none(), "candidate stays privately owned until release");
    update.preview.unwrap()
}
fn fixture_commit(service: &mut LayoutService, layout: &HostLayout, candidate: PreparedHostLayout) -> HostLayout {
    let expected = candidate.layout().clone();
    let prepared = candidate.into_prepared(service, &fixture_scope(service), layout).unwrap();
    service.commit(prepared).unwrap();
    let actual = resolve_host_layout(service, layout.capacity, layout.direction, layout.metrics, &layout.geometry_policy).unwrap();
    assert_eq!(actual.sections, expected.sections);
    assert_eq!(actual.allocation_environment, expected.allocation_environment);
    assert_eq!(actual.usable, expected.usable);
    actual
}

fn allocation_service(edit: impl Fn(&mut PanelSpec)) -> LayoutService {
    let mut registry = PanelRegistry::default();
    for original in synthetic_registry().unwrap().specs() {
        let mut spec = original.clone();
        edit(&mut spec);
        registry.register(spec).unwrap();
    }
    LayoutService::new(registry, Layout::empty(WorkspaceId::new("fixture.workspace").unwrap()), HostCapabilities::default()).unwrap()
}

#[test]
fn allocation_columns_have_independent_widths_and_mirror_outer_order_in_logical_units() {
    for zone in [Zone::Left, Zone::Right] {
        let mut service = fixture_service(false).unwrap();
        apply_fixture(
            &mut service,
            (0..3).map(|index| fixture_open("fixture.log", &format!("p{index}"), &format!("s{index}"), zone, index).unwrap()).collect(),
        );
        for (instance, target, column) in [("p1", "s0", "column.one"), ("p2", "s1", "column.two")] {
            apply_fixture(
                &mut service,
                vec![Mutation::Move {
                    instance: id(instance),
                    destination: Destination::Relative {
                        section: section(target),
                        placement: RelativePlacement::AfterColumn,
                        new_section: section(&instance.replace('p', "s")),
                        new_column: Some(ColumnId::new(column).unwrap()),
                        size: Size { width: 280.0, height: 240.0 },
                    },
                }],
            );
        }
        let widths = [200.0, 260.0, 320.0];
        let mutations = service
            .layout()
            .columns
            .iter()
            .zip(widths)
            .map(|(column, width)| Mutation::SetColumnWidth { column: column.id.clone(), width })
            .collect();
        apply_fixture(&mut service, mutations);
        let before = service.inspect();
        for scale in [1.0, 2.0] {
            for direction in [Direction::Ltr, Direction::Rtl] {
                let host = allocate_fixture(&service, Size { width: 1600.0, height: 900.0 }, direction, &fixture_metric_policy(&service));
                assert_eq!(host.sections.len(), 3);
                let physical_left = direction.edge(zone).unwrap() == PhysicalEdge::Left;
                for (index, card) in host.sections.iter().enumerate() {
                    assert_eq!(card.outer.size.width, widths[index]);
                    assert_eq!(card.outer.origin.y, host.capacity.origin.y);
                    if index > 0 {
                        let previous = host.sections[index - 1].outer;
                        assert!(if physical_left {
                            previous.origin.x + previous.size.width < card.outer.origin.x
                        } else {
                            card.outer.origin.x + card.outer.size.width < previous.origin.x
                        });
                    }
                    assert_eq!((card.outer.size.width * scale) / scale, widths[index]);
                }
                assert_eq!(host.viewport, Some(host.render_domain));
            }
        }
        assert_eq!(service.inspect(), before);
    }
}

#[test]
fn allocation_intersects_inactive_tab_and_every_stacked_section_width() {
    let mut service = allocation_service(|spec| {
        if spec.kind.0 == "fixture.application" {
            spec.min_size.width = 250.0;
            spec.max_size.width = 300.0;
        }
    });
    apply_fixture(
        &mut service,
        vec![
            fixture_open("fixture.log", "one", "one", Zone::Left, 0).unwrap(),
            fixture_open("fixture.log", "two", "two", Zone::Left, 1).unwrap(),
            Mutation::Open {
                kind: PanelKind::new("fixture.application").unwrap(),
                instance: id("inactive"),
                destination: Some(Destination::JoinTab { section: section("one"), index: 1 }),
            },
        ],
    );
    let column = service.layout().sections[0].column.clone().unwrap();
    assert_eq!(column_bounds(service.layout(), service.registry(), &column).unwrap(), (250.0, 300.0));
    let before = service.inspect();
    assert!(service.prepare(transaction(vec![Mutation::SetColumnWidth { column: column.clone(), width: 310.0 }])).is_err());
    assert_eq!(service.inspect(), before);
    apply_fixture(&mut service, vec![Mutation::SetColumnWidth { column, width: 250.0 }]);
    let host = allocate_fixture(&service, Size { width: 1200.0, height: 800.0 }, Direction::Ltr, &fixture_metric_policy(&service));
    assert!(host.sections.iter().all(|card| card.outer.size.width == 250.0));
}

#[test]
fn allocation_sole_content_uses_assigned_width_metrics_and_fill_leaves_no_false_gap() {
    for zone in [Zone::Left, Zone::Right] {
        let mut service = fixture_service(false).unwrap();
        apply_fixture(&mut service, vec![fixture_open("fixture.log", "one", "one", zone, 0).unwrap()]);
        let snapshot = |policy| HostGeometryPolicy {
            content_revision: 11,
            measure: Some(std::sync::Arc::new(AllocationMetricsFixture {
                revision: 11,
                policies: [(id("one"), policy)].into(),
                width: None,
                narrow_height: 360.0,
                wide_height: 180.0,
            })),
            ..HostGeometryPolicy::default()
        };
        for direction in [Direction::Ltr, Direction::Rtl] {
            let content = allocate_fixture(&service, Size { width: 1200.0, height: 800.0 }, direction, &snapshot(SideHeightPolicy::Content));
            assert_eq!(content.sections[0].outer.size.height, 360.0);
            let below = Point { x: middle(content.sections[0].outer).x, y: 600.0 };
            assert!(!content.consumes_pointer(below));
            assert!(content.usable.free.iter().any(|free| free.contains(below)));
            let fill = allocate_fixture(&service, content.capacity.size, direction, &snapshot(SideHeightPolicy::Fill));
            assert_eq!(fill.sections[0].outer.size.height, 800.0);
            assert!(fill.consumes_pointer(below));
        }
        let column = service.layout().sections[0].column.clone().unwrap();
        apply_fixture(&mut service, vec![Mutation::SetColumnWidth { column, width: 420.0 }]);
        let wide = allocate_fixture(&service, Size { width: 1200.0, height: 800.0 }, Direction::Ltr, &snapshot(SideHeightPolicy::Content));
        assert_eq!(wide.sections[0].outer.size.height, 180.0);
        assert_eq!(wide.allocation_environment.content[0].request.width, 420.0);
    }
}

#[test]
fn allocation_active_available_tab_owns_policy_while_inactive_height_limit_still_applies() {
    let mut service = allocation_service(|spec| {
        if spec.kind.0 == "fixture.application" {
            spec.max_size.height = 260.0;
        }
    });
    apply_fixture(
        &mut service,
        vec![
            fixture_open("fixture.log", "content", "group", Zone::Left, 0).unwrap(),
            Mutation::Open {
                kind: PanelKind::new("fixture.application").unwrap(),
                instance: id("fill"),
                destination: Some(Destination::JoinTab { section: section("group"), index: 1 }),
            },
        ],
    );
    let policy = HostGeometryPolicy {
        content_revision: 3,
        measure: Some(std::sync::Arc::new(AllocationMetricsFixture {
            revision: 3,
            policies: [(id("fill"), SideHeightPolicy::Fill)].into(),
            width: None,
            narrow_height: 180.0,
            wide_height: 180.0,
        })),
        ..HostGeometryPolicy::default()
    };
    let content = allocate_fixture(&service, Size { width: 1000.0, height: 800.0 }, Direction::Ltr, &policy);
    assert_eq!(content.sections[0].outer.size.height, 180.0);
    apply_fixture(&mut service, vec![Mutation::Activate { instance: id("fill") }]);
    let fill = allocate_fixture(&service, content.capacity.size, Direction::Ltr, &policy);
    assert_eq!(fill.sections[0].outer.size.height, 260.0);
    service.set_availability([(id("fill"), Availability::Unavailable { reason: "fixture context".into() })].into()).unwrap();
    let available = allocate_fixture(&service, content.capacity.size, Direction::Ltr, &policy);
    assert_eq!(available.sections[0].instance, id("content"));
    assert_eq!(available.sections[0].outer.size.height, 180.0);
}

#[test]
fn allocation_missing_or_stale_width_metrics_reject_before_any_candidate_is_shown() {
    let mut service = fixture_service(false).unwrap();
    apply_fixture(&mut service, vec![fixture_open("fixture.log", "one", "one", Zone::Left, 0).unwrap()]);
    let capacity = LogicalRect { origin: Point { x: 0.0, y: 0.0 }, size: Size { width: 1200.0, height: 800.0 } };
    let metrics = HostMetrics::default();
    assert_eq!(resolve_host_layout(&service, capacity, Direction::Ltr, metrics, &HostGeometryPolicy::default()).unwrap_err().rule, Rule::Revision);
    let base = fixture_metric_policy(&service);
    let request = host_measure_requests(&service, service.layout(), capacity, Direction::Ltr, metrics, &base).unwrap().remove(0);
    let measured = base.measure.as_ref().unwrap().measure(&request).unwrap();
    let policy = HostGeometryPolicy {
        content: vec![measured],
        content_revision: 7,
        identity_reservations: fixture_complete_reservations(&service),
        ..HostGeometryPolicy::default()
    };
    let host = resolve_host_layout(&service, capacity, Direction::Ltr, metrics, &policy).unwrap();
    let column = service.layout().sections[0].column.clone().unwrap();
    let before = service.inspect();
    let action = transaction(vec![Mutation::SetColumnWidth { column, width: 360.0 }]);
    assert_eq!(prepare_host_layout(&service, &fixture_scope(&service), &host, action).unwrap_err().rule, Rule::Revision);
    assert_eq!(service.inspect(), before);
    let mut stale = base;
    stale.content_revision = 8;
    assert_eq!(resolve_host_layout(&service, capacity, Direction::Ltr, metrics, &stale).unwrap_err().rule, Rule::Revision);
}

#[test]
fn allocation_collapse_restores_weights_and_max_limited_or_all_collapsed_stacks_leave_real_gaps() {
    let mut service = allocation_service(|spec| {
        spec.max_size.height = 240.0;
    });
    apply_fixture(
        &mut service,
        vec![fixture_open("fixture.log", "one", "one", Zone::Left, 0).unwrap(), fixture_open("fixture.log", "two", "two", Zone::Left, 1).unwrap()],
    );
    let policy = fixture_metric_policy(&service);
    let before = service.layout().clone();
    let expanded = allocate_fixture(&service, Size { width: 1200.0, height: 900.0 }, Direction::Ltr, &policy);
    assert!(expanded.sections.iter().all(|card| card.outer.size.height == 240.0));
    assert!(!expanded.consumes_pointer(Point { x: 80.0, y: 800.0 }));
    apply_fixture(
        &mut service,
        vec![Mutation::Collapse { section: section("one"), collapsed: true }, Mutation::Collapse { section: section("two"), collapsed: true }],
    );
    let collapsed = allocate_fixture(&service, expanded.capacity.size, Direction::Ltr, &policy);
    assert!(collapsed.rails.is_empty() && collapsed.resolved.rails.is_empty());
    assert!(collapsed.sections.iter().all(|card| card.collapsed
        && card.body.is_none()
        && card.footer.is_none()
        && card.outer.size.height == collapsed.metrics.header_height
        && card.outer.size.width == 280.0));
    assert!(!collapsed.consumes_pointer(Point { x: 80.0, y: 200.0 }));
    apply_fixture(
        &mut service,
        vec![Mutation::Collapse { section: section("one"), collapsed: false }, Mutation::Collapse { section: section("two"), collapsed: false }],
    );
    assert_eq!(service.layout().sections, before.sections);
    let restored = allocate_fixture(&service, expanded.capacity.size, Direction::Ltr, &policy);
    assert_eq!(restored.sections, expanded.sections);
}

#[test]
fn allocation_sole_transition_preserves_nonuniform_stack_weights() {
    let mut initial = fixture_service(false).unwrap();
    apply_fixture(
        &mut initial,
        vec![fixture_open("fixture.log", "one", "one", Zone::Left, 0).unwrap(), fixture_open("fixture.log", "two", "two", Zone::Left, 1).unwrap()],
    );
    let mut layout = initial.layout().clone();
    layout.sections[0].weight = 1.0;
    layout.sections[1].weight = 3.0;
    let mut service = LayoutService::new(synthetic_registry().unwrap(), layout, HostCapabilities::default()).unwrap();
    let before = allocate_fixture(&service, Size { width: 1200.0, height: 800.0 }, Direction::Ltr, &fixture_metric_policy(&service));
    assert!(before.sections[1].outer.size.height > before.sections[0].outer.size.height);
    service.set_availability([(id("two"), Availability::Unavailable { reason: "hidden context".into() })].into()).unwrap();
    let sole = allocate_fixture(&service, before.capacity.size, Direction::Ltr, &fixture_metric_policy(&service));
    assert_eq!(sole.sections.len(), 1);
    assert_eq!(sole.sections[0].outer.size.height, 240.0);
    assert_eq!(service.layout().sections[1].weight, 3.0);
    service.set_availability(AvailabilityMap::new()).unwrap();
    let restored = allocate_fixture(&service, before.capacity.size, Direction::Ltr, &fixture_metric_policy(&service));
    assert_eq!(restored.sections, before.sections);
}

#[test]
fn allocation_tiny_and_zero_capacities_retain_nonoverlapping_minima_and_truthful_clips() {
    let mut service = fixture_service(false).unwrap();
    apply_fixture(
        &mut service,
        vec![
            fixture_open("fixture.log", "left", "left", Zone::Left, 0).unwrap(),
            fixture_open("fixture.log", "right", "right", Zone::Right, 1).unwrap(),
            fixture_open("fixture.log", "bottom", "bottom", Zone::Bottom, 2).unwrap(),
        ],
    );
    let before = service.inspect();
    for direction in [Direction::Ltr, Direction::Rtl] {
        for scale in [1.0, 2.0] {
            for size in [Size { width: 100.0, height: 90.0 }, Size { width: 0.0, height: 0.0 }] {
                let host = allocate_fixture(&service, size, direction, &fixture_metric_policy(&service));
                assert_eq!(host.sections.len(), 3);
                assert!(host.extent.width > size.width && host.extent.height > size.height);
                for (index, card) in host.sections.iter().enumerate() {
                    assert!(card.outer.size.width >= 160.0 && card.outer.size.height >= 120.0);
                    assert_eq!((card.outer.size.width * scale) / scale, card.outer.size.width);
                    for other in host.sections.iter().skip(index + 1) {
                        assert!(!egui_rect(card.outer).intersect(egui_rect(other.outer)).is_positive());
                    }
                }
                assert!(host.painted.iter().chain(&host.input).all(|bounds| host.capacity.contains_rect(*bounds)));
                if size.width == 0.0 {
                    assert!(host.painted.is_empty() && host.controls.is_empty() && host.viewport.is_none());
                }
                let bottom = host.sections.iter().find(|card| card.zone == Zone::Bottom).unwrap();
                assert_eq!(bottom.outer.origin.y + bottom.outer.size.height, host.capacity.origin.y + host.extent.height);
            }
        }
    }
    assert_eq!(service.inspect(), before);
}

#[test]
fn allocation_scroll_changes_clipped_footprints_without_changing_extent_or_render_domain() {
    let mut service = fixture_service(false).unwrap();
    apply_fixture(
        &mut service,
        vec![
            fixture_open("fixture.log", "left", "left", Zone::Left, 0).unwrap(),
            fixture_open("fixture.log", "right", "right", Zone::Right, 1).unwrap(),
        ],
    );
    let policy = fixture_metric_policy(&service);
    let before = allocate_fixture(&service, Size { width: 100.0, height: 80.0 }, Direction::Ltr, &policy);
    let scrolled =
        allocate_fixture(&service, before.capacity.size, Direction::Ltr, &HostGeometryPolicy { scroll: Point { x: 999.0, y: 999.0 }, ..policy });
    assert_eq!(before.extent, scrolled.extent);
    assert_eq!(before.render_domain, scrolled.render_domain);
    assert!(scrolled.allocation_environment.scroll.x > 0.0);
    assert_ne!(before.sections[0].outer.origin, scrolled.sections[0].outer.origin);
    assert!(scrolled.painted.iter().all(|rect| scrolled.capacity.contains_rect(*rect)));
}

#[test]
fn allocation_owned_drop_candidate_is_observational_selected_only_and_commits_exact_geometry_once() {
    for direction in [Direction::Ltr, Direction::Rtl] {
        let mut service = fixture_service(false).unwrap();
        apply_fixture(
            &mut service,
            vec![
                fixture_open("fixture.log", "target", "target", Zone::Left, 0).unwrap(),
                fixture_open("fixture.log", "moving", "moving", Zone::Floating, 1).unwrap(),
            ],
        );
        let host = allocate_fixture(&service, Size { width: 1400.0, height: 900.0 }, direction, &fixture_metric_policy(&service));
        let before = service.inspect();
        let mut interaction = HostInteraction::default();
        fixture_press(&service, &host, &mut interaction, HostPointerTarget::Header { instance: id("moving") });
        assert!(interaction.preview().is_none());
        let target = host.sections.iter().find(|card| card.instance == id("target")).unwrap();
        let at = middle(target.outer);
        let preview = fixture_move(&service, &host, &mut interaction, at);
        assert!(preview.rejection.is_none(), "{preview:?}");
        assert_eq!(preview.targets.len(), 1, "only the selected successful landing is paintable");
        assert!(preview.tab_marker.is_some());
        assert_eq!(service.inspect(), before, "preview does not consume IDs or alter selection");
        let shown = preview.projected.clone().unwrap();
        let release = interaction.update(&service, &fixture_scope(&service), &host, HostPointerEvent::Release { at });
        assert!(release.action.is_none(), "runtime must consume the owned candidate");
        let candidate = release.candidate.unwrap();
        assert_eq!(candidate.layout(), &shown);
        assert_eq!(candidate.prepared().layout().section(&section("target")).unwrap().active, id("moving"));
        let committed = fixture_commit(&mut service, &host, candidate);
        assert_eq!(committed.sections, shown.sections);
        assert!(interaction.update(&service, &fixture_scope(&service), &committed, HostPointerEvent::Release { at }).candidate.is_none());
        assert_eq!(service.inspect().revision, before.revision + 1);
    }
}

#[test]
fn allocation_repeated_column_previews_do_not_drift_proposed_section_or_column_ids() {
    let mut service = fixture_service(false).unwrap();
    apply_fixture(
        &mut service,
        vec![
            fixture_open("fixture.log", "target", "target", Zone::Right, 0).unwrap(),
            fixture_open("fixture.log", "moving", "moving", Zone::Floating, 1).unwrap(),
        ],
    );
    let host = allocate_fixture(&service, Size { width: 1600.0, height: 900.0 }, Direction::Ltr, &fixture_metric_policy(&service));
    let before = service.inspect();
    let mut interaction = HostInteraction::default();
    let target = host.sections.iter().find(|card| card.instance == id("target")).unwrap();
    let at = Point { x: target.outer.origin.x + 2.0, y: middle(target.outer).y };
    let mut last = None;
    for _ in 0..8 {
        fixture_press(&service, &host, &mut interaction, HostPointerTarget::Header { instance: id("moving") });
        let preview = fixture_move(&service, &host, &mut interaction, at);
        assert!(preview.rejection.is_none(), "{preview:?}");
        if let Some(old) = &last {
            assert_eq!(old, &preview);
        }
        last = Some(preview);
        interaction.cancel();
        assert_eq!(service.inspect(), before);
    }
    fixture_press(&service, &host, &mut interaction, HostPointerTarget::Header { instance: id("moving") });
    let preview = fixture_move(&service, &host, &mut interaction, at);
    assert_eq!(last, Some(preview));
    let release = interaction.update(&service, &fixture_scope(&service), &host, HostPointerEvent::Release { at });
    let after = fixture_commit(&mut service, &host, release.candidate.unwrap());
    let moved = after.sections.iter().find(|card| card.instance == id("moving")).unwrap();
    assert!(moved.outer.origin.x < target.outer.origin.x, "physical left of a right dock stays physical left");
}

#[test]
fn live_drag_keeps_the_receiving_tab_active_and_stacks_in_the_lower_third() {
    for direction in [Direction::Ltr, Direction::Rtl] {
        let mut service = fixture_service(false).unwrap();
        apply_fixture(
            &mut service,
            vec![
                fixture_open("fixture.log", "target", "target", Zone::Left, 0).unwrap(),
                fixture_open("fixture.log", "moving", "moving", Zone::Floating, 1).unwrap(),
            ],
        );
        let host = allocate_fixture(&service, Size { width: 1400.0, height: 900.0 }, direction, &fixture_metric_policy(&service));
        let mut interaction = HostInteraction::default();
        interaction.set_live_drag(true);
        fixture_press(&service, &host, &mut interaction, HostPointerTarget::Header { instance: id("moving") });
        let receiver = host.sections.iter().find(|card| card.instance == id("target")).unwrap();
        let below = Point {
            x: receiver.outer.origin.x + if direction == Direction::Ltr { receiver.outer.size.width - 4.0 } else { 4.0 },
            y: receiver.outer.origin.y + receiver.outer.size.height * 0.7,
        };
        let preview = fixture_move(&service, &host, &mut interaction, below);
        assert!(matches!(preview.destination, Some(Destination::Relative { placement: RelativePlacement::Below, .. })), "{preview:?}");
        let at = middle(receiver.outer);
        let preview = fixture_move(&service, &host, &mut interaction, at);
        assert!(matches!(preview.destination, Some(Destination::JoinTab { .. })), "{preview:?}");
        assert!(preview.tab_marker.is_some());
        assert_eq!(preview.projected.unwrap().sections[0].instance, id("target"));
        let release = interaction.update(&service, &fixture_scope(&service), &host, HostPointerEvent::Release { at });
        let candidate = release.candidate.unwrap();
        assert_eq!(candidate.prepared().layout().section(&section("target")).unwrap().active, id("target"));
        fixture_commit(&mut service, &host, candidate);
        assert_eq!(service.layout().section(&section("target")).unwrap().tabs, [id("target"), id("moving")]);
    }
}

#[test]
fn detached_drag_preview_reflows_panels_without_committing_a_close() {
    let content = host_logs(2, Zone::Left);
    let mut raw = RawPanelHost::new(content, 1.0);
    raw.frame(Vec::new(), false);
    let frame = raw.host.frame().unwrap();
    let service = raw.content.runtime.layout();
    let before = service.inspect();
    let instance = id("log-0");
    let scope = fixture_scope(service);
    let detached = super::interaction::detached_drag_layout(service, &scope, &frame.layout, &instance).unwrap();
    let closed = prepare_host_layout(
        service,
        &scope,
        &frame.layout,
        LayoutAction::Apply { expected_revision: Some(before.revision), mutations: vec![Mutation::Close { instance }] },
    )
    .unwrap();
    assert_eq!(detached.sections, closed.layout().sections);
    assert_eq!(detached.sections.len(), 1);
    assert_eq!(service.inspect(), before);
}

#[test]
fn allocation_invisible_physical_edges_and_measured_tab_halves_preserve_rtl_and_detached_indices() {
    for zone in [Zone::Left, Zone::Right] {
        for direction in [Direction::Ltr, Direction::Rtl] {
            let mut service = fixture_service(false).unwrap();
            apply_fixture(
                &mut service,
                vec![
                    fixture_open("fixture.log", "first", "group", zone, 0).unwrap(),
                    Mutation::Open {
                        kind: PanelKind::new("fixture.log").unwrap(),
                        instance: id("moving"),
                        destination: Some(Destination::JoinTab { section: section("group"), index: 1 }),
                    },
                    Mutation::Open {
                        kind: PanelKind::new("fixture.log").unwrap(),
                        instance: id("last"),
                        destination: Some(Destination::JoinTab { section: section("group"), index: 2 }),
                    },
                ],
            );
            let column = service.layout().section(&section("group")).unwrap().column.clone().unwrap();
            apply_fixture(&mut service, vec![Mutation::SetColumnWidth { column, width: 410.0 }]);
            let mut policy = fixture_metric_policy(&service);
            policy.measure = Some(std::sync::Arc::new(AllocationMetricsFixture {
                revision: 7,
                policies: Default::default(),
                width: None,
                narrow_height: 280.0,
                wide_height: 280.0,
            }));
            let host = allocate_fixture(&service, Size { width: 1400.0, height: 900.0 }, direction, &policy);
            let card = &host.sections[0];
            let targets = host_drop_targets(&service, &host, &id("moving")).unwrap();
            let strip = card.tab_strip.unwrap();
            assert!(
                targets
                    .iter()
                    .filter(|target| target.priority == 2)
                    .all(|target| target.bounds.origin.y == strip.origin.y + 9.0 && target.bounds.size.height == (strip.size.height - 18.0).max(1.0))
            );
            assert!(
                targets
                    .iter()
                    .any(|target| target.stable_key == "tab:group:1"
                        && target.destination == Destination::JoinTab { section: section("group"), index: 1 }),
                "keep the source tab's own insertion slot after detachment"
            );
            let physical_left = direction.edge(zone).unwrap() == PhysicalEdge::Left;
            for left in [true, false] {
                let placement = if left == physical_left { RelativePlacement::BeforeColumn } else { RelativePlacement::AfterColumn };
                let target = targets.iter().find(|target| target.id == HostDropTargetId::Relative { section: section("group"), placement }).unwrap();
                let inward = 64.0_f32.min(card.outer.size.width * 0.28);
                let outward = if placement == RelativePlacement::AfterColumn { 64.0 } else { 18.0 };
                assert_eq!(target.bounds.size.width, inward + outward);
                assert_eq!(
                    target.bounds.origin.x,
                    if left { card.outer.origin.x - outward } else { card.outer.origin.x + card.outer.size.width - inward }
                );
            }
            let before_moving = HostDropTargetId::Tab { section: section("group"), before: Some(id("moving")) };
            let first = card.tabs.iter().find(|tab| tab.instance == id("first")).unwrap();
            let moving = card.tabs.iter().find(|tab| tab.instance == id("moving")).unwrap();
            let first_trailing = Point {
                x: first.bounds.origin.x + first.bounds.size.width * if direction == Direction::Ltr { 0.75 } else { 0.25 },
                y: middle(first.bounds).y,
            };
            let moving_leading = Point {
                x: moving.bounds.origin.x + moving.bounds.size.width * if direction == Direction::Ltr { 0.25 } else { 0.75 },
                y: middle(moving.bounds).y,
            };
            let mut interaction = HostInteraction::default();
            fixture_press(&service, &host, &mut interaction, HostPointerTarget::Tab { instance: id("last") });
            let preview = fixture_move(&service, &host, &mut interaction, first_trailing);
            assert_eq!(preview.target, Some(before_moving.clone()), "the trailing half inserts after the first tab");
            assert_eq!(preview.destination, Some(Destination::JoinTab { section: section("group"), index: 1 }));
            assert_eq!(preview.targets.len(), 1);
            let release = interaction.update(&service, &fixture_scope(&service), &host, HostPointerEvent::Release { at: moving_leading });
            assert_eq!(release.preview.unwrap().target, Some(before_moving));
            assert_eq!(release.candidate.unwrap().layout(), preview.projected.as_ref().unwrap());
            fixture_press(&service, &host, &mut interaction, HostPointerTarget::Tab { instance: id("moving") });
            let corner = Point {
                x: if physical_left { card.outer.origin.x + 45.0 } else { card.outer.origin.x + card.outer.size.width - 45.0 },
                y: card.outer.origin.y + 45.0,
            };
            // Both edge distances are 45 and both region areas are 22960:
            // column:group:* must sort before stack:group:above in LTR/RTL.
            assert_eq!(card.outer.size, Size { width: 410.0, height: 280.0 });
            let preview = fixture_move(&service, &host, &mut interaction, corner);
            assert!(preview.rejection.is_none(), "{preview:?}");
            assert_eq!(preview.target, Some(HostDropTargetId::Relative { section: section("group"), placement: RelativePlacement::BeforeColumn }));
            interaction.cancel();
            fixture_press(&service, &host, &mut interaction, HostPointerTarget::Tab { instance: id("moving") });
            let inner_corner = Point { x: if physical_left { right(card.outer) - 45.0 } else { card.outer.origin.x + 45.0 }, y: corner.y };
            assert_eq!(
                fixture_move(&service, &host, &mut interaction, inner_corner).target,
                Some(HostDropTargetId::Relative { section: section("group"), placement: RelativePlacement::AfterColumn }),
                "the wider centre-facing strip keeps the original equal-distance/area tie"
            );
        }
    }
}

#[test]
fn allocation_shared_shell_gap_controls_allocation_and_keeps_compact_sashes_and_free_origins() {
    for mode in [crate::ui_kit::LayoutMode::Docked, crate::ui_kit::LayoutMode::Floating] {
        for direction in [Direction::Ltr, Direction::Rtl] {
            for zone in [Zone::Left, Zone::Right] {
                let gap = crate::ui_kit::Container::for_layout(mode, crate::ui_kit::Surface::Panel).gap();
                let mut service = fixture_service(false).unwrap();
                apply_fixture(
                    &mut service,
                    vec![
                        fixture_open("fixture.log", "a", "a", zone, 0).unwrap(),
                        fixture_open("fixture.log", "b", "b", zone, 1).unwrap(),
                        fixture_open("fixture.log", "c", "c", zone, 2).unwrap(),
                    ],
                );
                apply_fixture(
                    &mut service,
                    vec![
                        Mutation::Move {
                            instance: id("c"),
                            destination: Destination::Relative {
                                section: section("a"),
                                placement: RelativePlacement::AfterColumn,
                                new_section: section("c"),
                                new_column: Some(ColumnId::new("inner").unwrap()),
                                size: Size { width: 280.0, height: 240.0 },
                            },
                        },
                        Mutation::Open {
                            kind: PanelKind::new("fixture.log").unwrap(),
                            instance: id("free"),
                            destination: Some(Destination::NewSection {
                                section: section("free"),
                                index: 3,
                                location: SectionLocation::Floating { origin: Point { x: 500.0, y: 300.0 } },
                                size: Size { width: 280.0, height: 240.0 },
                            }),
                        },
                    ],
                );
                let before = service.inspect();
                let domain = LogicalRect { origin: Point { x: 0.0, y: 30.0 }, size: Size { width: 1400.0, height: 900.0 } };
                let capacity = LogicalRect {
                    origin: Point { x: gap, y: domain.origin.y + gap },
                    size: Size { width: domain.size.width - 2.0 * gap, height: domain.size.height - 2.0 * gap },
                };
                let metrics = HostMetrics { gap, ..HostMetrics::default() };
                let host =
                    resolve_host_layout_in_domain(&service, domain, capacity, &[], direction, metrics, &fixture_metric_policy(&service)).unwrap();
                let card = |name| host.sections.iter().find(|card| card.section == section(name)).unwrap().outer;
                let (a, b, c) = (card("a"), card("b"), card("c"));
                assert_eq!(b.origin.y - (a.origin.y + a.size.height), gap);
                let (left, right) = if a.origin.x < c.origin.x { (a, c) } else { (c, a) };
                assert_eq!(right.origin.x - (left.origin.x + left.size.width), gap);
                assert_eq!(host.allocation_environment.gap, gap);
                assert_eq!(host.metrics.sash_width, HostMetrics::default().sash_width);
                assert_eq!(card("free").origin, Point { x: 500.0, y: 300.0 });
                let sash = host.sashes.first().unwrap();
                assert!(sash.bounds.size.height > 0.0, "joined compact cards retain a real resize control");
                assert_eq!(host.hit_test(middle(sash.bounds)), Some(HostPointerTarget::Sash { first: section("a"), second: section("b") }));
                if gap > 0.0 {
                    let gutter = LogicalRect {
                        origin: Point { x: left.origin.x + left.size.width + 1.0, y: capacity.origin.y + 80.0 },
                        size: Size { width: gap - 2.0, height: 4.0 },
                    };
                    assert!(host.usable.contains_rect(gutter));
                    assert!(!host.consumes_pointer(middle(gutter)));
                }
                assert_eq!(host.viewport, Some(domain));
                assert_eq!(service.inspect(), before, "shell fitting never rewrites free placement intent");
            }
        }
    }
}

#[test]
fn allocation_workspace_edge_gutters_admit_owned_drops_while_chrome_and_hidden_cards_do_not() {
    for direction in [Direction::Ltr, Direction::Rtl] {
        for zone in [Zone::Left, Zone::Right] {
            let mut service = fixture_service(false).unwrap();
            apply_fixture(
                &mut service,
                vec![
                    fixture_open("fixture.log", "target", "target", zone, 0).unwrap(),
                    fixture_open("fixture.log", "moving", "moving", Zone::Floating, 1).unwrap(),
                ],
            );
            let domain = LogicalRect { origin: Point { x: 10.0, y: 20.0 }, size: Size { width: 1400.0, height: 900.0 } };
            let capacity = LogicalRect { origin: Point { x: 17.0, y: 100.0 }, size: Size { width: 1386.0, height: 813.0 } };
            let chrome = [
                LogicalRect { origin: domain.origin, size: Size { width: 1400.0, height: 73.0 } },
                LogicalRect { origin: Point { x: 10.0, y: 900.0 }, size: Size { width: 1400.0, height: 20.0 } },
            ];
            let host = resolve_host_layout_in_domain(
                &service,
                domain,
                capacity,
                &chrome,
                direction,
                HostMetrics { gap: 7.0, ..HostMetrics::default() },
                &fixture_metric_policy(&service),
            )
            .unwrap();
            let at = Point { x: if direction.edge(zone).unwrap() == PhysicalEdge::Left { 11.0 } else { 1409.0 }, y: 650.0 };
            assert!(!capacity.contains(at));
            assert!(!host.consumes_pointer(at), "workspace bands remain invisible data");
            let before = service.inspect();
            let mut interaction = HostInteraction::default();
            fixture_press(&service, &host, &mut interaction, HostPointerTarget::Header { instance: id("moving") });
            let preview = fixture_move(&service, &host, &mut interaction, at);
            assert!(preview.rejection.is_none(), "{preview:?}");
            assert_eq!(preview.target, Some(HostDropTargetId::Dock { zone, before: None }));
            let released = interaction.update(&service, &fixture_scope(&service), &host, HostPointerEvent::Release { at });
            assert_eq!(released.candidate.unwrap().layout(), preview.projected.as_ref().unwrap());
            fixture_press(&service, &host, &mut interaction, HostPointerTarget::Header { instance: id("moving") });
            let blocked = fixture_move(&service, &host, &mut interaction, Point { x: 700.0, y: 910.0 });
            assert!(blocked.action.is_none() && blocked.projected.is_none());
            assert!(
                interaction
                    .update(&service, &fixture_scope(&service), &host, HostPointerEvent::Release { at: Point { x: 700.0, y: 910.0 } })
                    .candidate
                    .is_none()
            );
            assert_eq!(service.inspect(), before);

            // Side bands stop above the final nine points, leaving both bottom
            // corners to Bottom even when the opposite side is empty.
            let no_status =
                resolve_host_layout_in_domain(&service, domain, capacity, &chrome[..1], direction, host.metrics, &fixture_metric_policy(&service))
                    .unwrap();
            for x in [domain.origin.x + 1.0, right(domain) - 1.0] {
                fixture_press(&service, &no_status, &mut interaction, HostPointerTarget::Header { instance: id("moving") });
                let preview = fixture_move(&service, &no_status, &mut interaction, Point { x, y: bottom(domain) - 1.0 });
                assert!(preview.rejection.is_none(), "{preview:?}");
                assert_eq!(preview.target, Some(HostDropTargetId::Dock { zone: Zone::Bottom, before: None }));
                interaction.cancel();
            }

            // An overflowed card above the visible capacity must not publish
            // tab/relative targets merely because it lies in the render domain.
            apply_fixture(&mut service, vec![fixture_open("fixture.log", "tail", "tail", zone, 2).unwrap()]);
            let mut small_capacity = capacity;
            small_capacity.size.height = 100.0;
            let mut policy = fixture_metric_policy(&service);
            policy.scroll.y = 147.0;
            let scrolled = resolve_host_layout_in_domain(&service, domain, small_capacity, &chrome, direction, host.metrics, &policy).unwrap();
            let targets = host_drop_targets(&service, &scrolled, &id("moving")).unwrap();
            assert!(intersect_rect(scrolled.sections.iter().find(|card| card.section == section("target")).unwrap().outer, small_capacity).is_none());
            assert!(!targets.iter().any(|target| matches!(&target.id, HostDropTargetId::Relative { section: id, .. } | HostDropTargetId::Tab { section: id, .. } if *id == section("target"))));
        }
    }
}

#[test]
fn allocation_centre_column_reach_uses_presented_receivers_after_source_removal() {
    for direction in [Direction::Ltr, Direction::Rtl] {
        for zone in [Zone::Left, Zone::Right] {
            for case in ["present", "closed", "unavailable", "offscreen", "vacating", "retained-tab", "unavailable-tab", "retained-card"] {
                let mut service = allocation_service(|spec| {
                    spec.max_size.height = 240.0;
                });
                apply_fixture(
                    &mut service,
                    vec![
                        fixture_open("fixture.log", "outer", "outer", zone, 0).unwrap(),
                        fixture_open("fixture.log", "tail", "tail", zone, 1).unwrap(),
                        fixture_open("fixture.log", "inner", "inner", zone, 2).unwrap(),
                        fixture_open("fixture.log", "moving", "moving", Zone::Floating, 3).unwrap(),
                    ],
                );
                apply_fixture(
                    &mut service,
                    vec![
                        Mutation::Move {
                            instance: id("inner"),
                            destination: Destination::Relative {
                                section: section("outer"),
                                placement: RelativePlacement::AfterColumn,
                                new_section: section("inner"),
                                new_column: Some(ColumnId::new("inner-column").unwrap()),
                                size: Size { width: 280.0, height: 240.0 },
                            },
                        },
                        Mutation::Collapse { section: section("inner"), collapsed: true },
                        Mutation::SetFloatingRect {
                            section: section("moving"),
                            origin: Point { x: 700.0, y: 600.0 },
                            size: Size { width: 280.0, height: 240.0 },
                        },
                    ],
                );
                match case {
                    "closed" => apply_fixture(&mut service, vec![Mutation::Close { instance: id("inner") }]),
                    "unavailable" => {
                        service.set_availability([(id("inner"), Availability::Unavailable { reason: "hidden fixture".into() })].into()).unwrap()
                    }
                    "retained-tab" | "unavailable-tab" => apply_fixture(
                        &mut service,
                        vec![Mutation::Open {
                            kind: PanelKind::new("fixture.log").unwrap(),
                            instance: id("peer"),
                            destination: Some(Destination::JoinTab { section: section("inner"), index: 1 }),
                        }],
                    ),
                    "retained-card" => apply_fixture(
                        &mut service,
                        vec![Mutation::Open {
                            kind: PanelKind::new("fixture.log").unwrap(),
                            instance: id("peer"),
                            destination: Some(Destination::Relative {
                                section: section("inner"),
                                placement: RelativePlacement::Below,
                                new_section: section("peer"),
                                new_column: None,
                                size: Size { width: 280.0, height: 240.0 },
                            }),
                        }],
                    ),
                    _ => {}
                }
                if case == "unavailable-tab" {
                    service.set_availability([(id("peer"), Availability::Unavailable { reason: "hidden retained tab".into() })].into()).unwrap();
                }
                let mut policy = fixture_metric_policy(&service);
                if case == "offscreen" {
                    policy.scroll.y = 260.0;
                }
                let host =
                    allocate_fixture(&service, Size { width: 1600.0, height: if case == "offscreen" { 200.0 } else { 900.0 } }, direction, &policy);
                if case == "closed" {
                    assert!(
                        service.layout().columns.iter().any(|column| column.id == ColumnId::new("inner-column").unwrap()),
                        "closed memory retains a dormant column"
                    );
                    assert!(!host.sections.iter().any(|card| card.section == section("inner")));
                }
                let moving =
                    if matches!(case, "vacating" | "retained-tab" | "unavailable-tab" | "retained-card") { id("inner") } else { id("moving") };
                let targets = host_drop_targets(&service, &host, &moving).unwrap();
                let tail = host.sections.iter().find(|card| card.section == section("tail")).unwrap();
                let edge = targets
                    .iter()
                    .find(|entry| entry.id == HostDropTargetId::Relative { section: section("tail"), placement: RelativePlacement::AfterColumn })
                    .unwrap();
                let outward = if matches!(case, "closed" | "unavailable" | "offscreen" | "vacating" | "unavailable-tab") { 64.0 } else { 18.0 };
                assert_eq!(edge.bounds.size.width, 64.0_f32.min(tail.outer.size.width * 0.28) + outward, "{direction:?}/{zone:?}/{case}");
                let before = targets
                    .iter()
                    .find(|entry| entry.id == HostDropTargetId::Relative { section: section("tail"), placement: RelativePlacement::BeforeColumn })
                    .unwrap();
                assert_eq!(before.bounds.size.width, 64.0_f32.min(tail.outer.size.width * 0.28) + 18.0);
                if case == "offscreen" {
                    let inner = host.sections.iter().find(|card| card.section == section("inner")).unwrap();
                    assert!(intersect_rect(inner.outer, host.capacity).is_none());
                    assert!(
                        !targets
                            .iter()
                            .any(|entry| matches!(&entry.id, HostDropTargetId::Relative { section: target, .. } if *target == section("inner")))
                    );
                }
                if matches!(case, "retained-tab" | "retained-card") {
                    let receiver = if case == "retained-tab" { "inner" } else { "peer" };
                    let card = host.sections.iter().find(|card| card.section == section(receiver)).unwrap();
                    let edge = targets
                        .iter()
                        .find(|entry| {
                            entry.id == HostDropTargetId::Relative { section: section(receiver), placement: RelativePlacement::AfterColumn }
                        })
                        .unwrap();
                    assert_eq!(edge.bounds.size.width, 64.0_f32.min(card.outer.size.width * 0.28) + 64.0);
                }
            }
        }
    }
}

#[test]
fn allocation_added_column_strip_stops_at_actual_paint_and_chrome_without_shrinking_old_band() {
    for direction in [Direction::Ltr, Direction::Rtl] {
        for zone in [Zone::Left, Zone::Right] {
            for (case, distance, outward) in [
                ("floating", 32.0, 32.0),
                ("collapsed", 40.0, 40.0),
                ("chrome", 48.0, 48.0),
                ("overlapping", 12.0, 18.0),
                ("distant", 80.0, 64.0),
                ("offscreen", 40.0, 64.0),
                ("other-column", 40.0, 40.0),
            ] {
                let mut service = fixture_service(false).unwrap();
                apply_fixture(
                    &mut service,
                    vec![
                        fixture_open("fixture.log", "target", "target", zone, 0).unwrap(),
                        fixture_open("fixture.log", "moving", "moving", Zone::Bottom, 1).unwrap(),
                    ],
                );
                let size = Size { width: if case == "other-column" { 640.0 } else { 1400.0 }, height: 900.0 };
                let base = allocate_fixture(&service, size, direction, &fixture_metric_policy(&service));
                let card = base.sections.iter().find(|card| card.section == section("target")).unwrap();
                let physical_left = direction.edge(zone).unwrap() == PhysicalEdge::Left;
                let at = if physical_left { right(card.outer) } else { card.outer.origin.x };
                let obstacle = LogicalRect {
                    origin: Point {
                        x: if physical_left { at + distance } else { at - distance - 160.0 },
                        y: if case == "offscreen" { 1000.0 } else { card.outer.origin.y + 80.0 },
                    },
                    size: Size { width: 160.0, height: 120.0 },
                };
                let mut chrome = Vec::new();
                if matches!(case, "chrome" | "offscreen") {
                    chrome.push(obstacle);
                } else if case == "other-column" {
                    let other = if zone == Zone::Left { Zone::Right } else { Zone::Left };
                    apply_fixture(&mut service, vec![fixture_open("fixture.log", "obstacle", "obstacle", other, 2).unwrap()]);
                    let column = service.layout().section(&section("obstacle")).unwrap().column.clone().unwrap();
                    apply_fixture(&mut service, vec![Mutation::SetColumnWidth { column, width: 320.0 }]);
                } else {
                    apply_fixture(
                        &mut service,
                        vec![Mutation::Open {
                            kind: PanelKind::new("fixture.log").unwrap(),
                            instance: id("obstacle"),
                            destination: Some(Destination::NewSection {
                                section: section("obstacle"),
                                index: 2,
                                location: SectionLocation::Floating { origin: obstacle.origin },
                                size: obstacle.size,
                            }),
                        }],
                    );
                    if case == "collapsed" {
                        apply_fixture(&mut service, vec![Mutation::Collapse { section: section("obstacle"), collapsed: true }]);
                    }
                }
                let host = resolve_host_layout_in_domain(
                    &service,
                    base.render_domain,
                    base.capacity,
                    &chrome,
                    direction,
                    base.metrics,
                    &fixture_metric_policy(&service),
                )
                .unwrap();
                let targets = host_drop_targets(&service, &host, &id("moving")).unwrap();
                let target = targets
                    .iter()
                    .find(|entry| entry.id == HostDropTargetId::Relative { section: section("target"), placement: RelativePlacement::AfterColumn })
                    .unwrap();
                assert_eq!(target.bounds.size.width, 64.0_f32.min(card.outer.size.width * 0.28) + outward, "{direction:?}/{zone:?}/{case}");
                assert_eq!(target.bounds.origin.x, if physical_left { at - 64.0_f32.min(card.outer.size.width * 0.28) } else { at - outward });
                if matches!(case, "floating" | "collapsed" | "chrome") {
                    let mut interaction = HostInteraction::default();
                    fixture_press(&service, &host, &mut interaction, HostPointerTarget::Header { instance: id("moving") });
                    let point = |distance: f32| Point { x: at + if physical_left { distance } else { -distance }, y: obstacle.origin.y + 1.0 };
                    assert_eq!(fixture_move(&service, &host, &mut interaction, point(outward - 1.0)).target, Some(target.id.clone()));
                    assert_ne!(
                        fixture_move(&service, &host, &mut interaction, point(outward)).target,
                        Some(target.id.clone()),
                        "actual obstacle boundary belongs to its paint"
                    );
                    assert_ne!(fixture_move(&service, &host, &mut interaction, point(outward + 1.0)).target, Some(target.id.clone()));
                }
            }
        }
    }
}

#[test]
fn allocation_non_move_controls_never_expose_drag_title_feedback() {
    let mut service = fixture_service(false).unwrap();
    apply_fixture(
        &mut service,
        vec![fixture_open("fixture.log", "one", "one", Zone::Left, 0).unwrap(), fixture_open("fixture.log", "two", "two", Zone::Left, 1).unwrap()],
    );
    let host = allocate_fixture(&service, Size { width: 1400.0, height: 900.0 }, Direction::Ltr, &fixture_metric_policy(&service));
    let before = service.inspect();
    let mut interaction = HostInteraction::default();
    let (mut resize, mut pair) = (false, false);
    for control in &host.controls {
        if matches!(control.target, HostPointerTarget::Header { .. } | HostPointerTarget::Tab { .. } | HostPointerTarget::Rail { .. }) {
            continue;
        }
        resize |= matches!(control.target, HostPointerTarget::Resize { .. });
        pair |= matches!(control.target, HostPointerTarget::Sash { .. });
        let start = fixture_press(&service, &host, &mut interaction, control.target.clone());
        interaction.update(&service, &fixture_scope(&service), &host, HostPointerEvent::Move { at: Point { x: start.x + 8.0, y: start.y + 8.0 } });
        assert!(interaction.move_source().is_none() && interaction.move_position().is_none(), "{:?}", control.target);
        interaction.cancel();
    }
    assert!(resize && pair, "exercise both resize capture kinds as well as ordinary controls");
    assert_eq!(service.inspect(), before);
}

#[test]
fn allocation_drag_threshold_scales_once_and_invalid_or_outside_release_discards_the_shown_candidate() {
    for scale in [1.0, 2.0] {
        let mut service = fixture_service(false).unwrap();
        apply_fixture(
            &mut service,
            vec![
                fixture_open("fixture.log", "one", "one", Zone::Left, 0).unwrap(),
                fixture_open("fixture.log", "moving", "moving", Zone::Floating, 1).unwrap(),
            ],
        );
        let host = allocate_fixture(&service, Size { width: 1400.0, height: 900.0 }, Direction::Ltr, &fixture_metric_policy(&service));
        let before = service.inspect();
        let mut interaction = HostInteraction::default();
        let start = fixture_press(&service, &host, &mut interaction, HostPointerTarget::Header { instance: id("moving") });
        let logical = |physical_dx: f32| Point { x: start.x + physical_dx / scale, y: start.y };
        assert!(interaction.move_source().is_none() && interaction.move_position().is_none());
        assert!(
            interaction.update(&service, &fixture_scope(&service), &host, HostPointerEvent::Move { at: logical(5.99 * scale) }).preview.is_none()
        );
        assert!(interaction.move_source().is_none() && interaction.move_position().is_none());
        assert!(interaction.update(&service, &fixture_scope(&service), &host, HostPointerEvent::Move { at: logical(6.0 * scale) }).preview.is_some());
        assert_eq!(interaction.move_source(), Some(&id("moving")));
        assert_eq!(interaction.move_position(), Some(logical(6.0 * scale)));
        let at = middle(host.sections.iter().find(|card| card.instance == id("one")).unwrap().outer);
        for release_at in [Point { x: 1200.0, y: 850.0 }, Point { x: -1.0, y: 300.0 }, Point { x: f32::NAN, y: 300.0 }] {
            fixture_press(&service, &host, &mut interaction, HostPointerTarget::Header { instance: id("moving") });
            let preview = fixture_move(&service, &host, &mut interaction, at);
            assert!(preview.rejection.is_none());
            let release = interaction.update(&service, &fixture_scope(&service), &host, HostPointerEvent::Release { at: release_at });
            assert!(release.candidate.is_none());
            assert!(!interaction.is_captured() && interaction.preview().is_none());
            assert!(interaction.move_source().is_none() && interaction.move_position().is_none());
            assert_eq!(service.inspect(), before, "release must re-hit-test even without a final Move");
        }
    }
}

#[test]
fn allocation_transient_leave_discards_candidates_but_revalidates_capture_and_hostile_moves() {
    for case in ["leave-release", "negative", "positive", "nan", "hostile", "stale"] {
        let mut service = fixture_service(false).unwrap();
        apply_fixture(
            &mut service,
            vec![
                fixture_open("fixture.log", "target", "target", Zone::Left, 0).unwrap(),
                fixture_open("fixture.log", "moving", "moving", Zone::Floating, 1).unwrap(),
            ],
        );
        let host = allocate_fixture(&service, Size { width: 1400.0, height: 900.0 }, Direction::Ltr, &fixture_metric_policy(&service));
        let at = middle(host.sections.iter().find(|card| card.instance == id("target")).unwrap().outer);
        let mut interaction = HostInteraction::default();
        fixture_press(&service, &host, &mut interaction, HostPointerTarget::Header { instance: id("moving") });
        assert!(fixture_move(&service, &host, &mut interaction, at).rejection.is_none());
        let event = match case {
            "negative" => HostPointerEvent::Move { at: Point { x: -20.0, y: -20.0 } },
            "positive" => HostPointerEvent::Move { at: Point { x: 1500.0, y: 1000.0 } },
            "nan" => HostPointerEvent::Move { at: Point { x: f32::NAN, y: at.y } },
            "hostile" => HostPointerEvent::Move { at: Point { x: MAX_LOGICAL + 1.0, y: at.y } },
            _ => HostPointerEvent::Leave,
        };
        let result = interaction.update(&service, &fixture_scope(&service), &host, event);
        assert!(result.preview.is_none() && result.candidate.is_none());
        assert_eq!(interaction.is_captured(), !matches!(case, "nan" | "hostile"));
        assert_eq!(interaction.move_source(), (!matches!(case, "nan" | "hostile")).then(|| id("moving")).as_ref());
        assert!(interaction.move_position().is_none());
        if case == "stale" {
            apply_fixture(&mut service, vec![Mutation::Focus { instance: id("target") }]);
            let result = interaction.update(&service, &fixture_scope(&service), &host, HostPointerEvent::Reconcile);
            assert!(result.rejection.is_some());
            assert!(!interaction.is_captured());
            assert!(interaction.move_source().is_none() && interaction.move_position().is_none());
        } else if matches!(case, "negative" | "positive") {
            let preview = fixture_move(&service, &host, &mut interaction, at);
            assert!(preview.rejection.is_none());
            assert_eq!(interaction.move_source(), Some(&id("moving")));
            assert_eq!(interaction.move_position(), Some(at));
            let release = interaction.update(&service, &fixture_scope(&service), &host, HostPointerEvent::Release { at });
            assert_eq!(release.candidate.as_ref().unwrap().layout(), preview.projected.as_ref().unwrap());
        } else {
            let before = service.inspect();
            assert!(interaction.update(&service, &fixture_scope(&service), &host, HostPointerEvent::Release { at }).candidate.is_none());
            assert_eq!(service.inspect(), before, "a suspended candidate cannot commit on reentry Release alone");
        }
    }
    // A fast first sample outside still crosses the drag threshold, even when
    // the pointer returns to its press position before another inside sample.
    for outside in [Point { x: -20.0, y: -20.0 }, Point { x: 1500.0, y: 1000.0 }] {
        let mut service = fixture_service(false).unwrap();
        apply_fixture(&mut service, vec![fixture_open("fixture.log", "moving", "moving", Zone::Left, 0).unwrap()]);
        let host = allocate_fixture(&service, Size { width: 1400.0, height: 900.0 }, Direction::Ltr, &fixture_metric_policy(&service));
        let mut interaction = HostInteraction::default();
        let start = fixture_press(&service, &host, &mut interaction, HostPointerTarget::Header { instance: id("moving") });
        assert!(interaction.update(&service, &fixture_scope(&service), &host, HostPointerEvent::Move { at: outside }).preview.is_none());
        assert_eq!(interaction.move_source(), Some(&id("moving")));
        assert!(interaction.move_position().is_none());
        assert!(interaction.update(&service, &fixture_scope(&service), &host, HostPointerEvent::Move { at: start }).preview.is_some());
    }
    for leave in [false, true] {
        let mut service = fixture_service(false).unwrap();
        apply_fixture(
            &mut service,
            vec![
                fixture_open("fixture.log", "target", "target", Zone::Left, 0).unwrap(),
                Mutation::Open {
                    kind: PanelKind::new("fixture.log").unwrap(),
                    instance: id("tab"),
                    destination: Some(Destination::JoinTab { section: section("target"), index: 1 }),
                },
            ],
        );
        assert_eq!(service.layout().section(&section("target")).unwrap().active, id("target"));
        let host = allocate_fixture(&service, Size { width: 1400.0, height: 900.0 }, Direction::Ltr, &fixture_metric_policy(&service));
        let mut interaction = HostInteraction::default();
        let start = fixture_press(&service, &host, &mut interaction, HostPointerTarget::Tab { instance: id("tab") });
        if leave {
            interaction.update(&service, &fixture_scope(&service), &host, HostPointerEvent::Leave);
            interaction.update(&service, &fixture_scope(&service), &host, HostPointerEvent::Move { at: start });
        }
        let released = interaction.update(&service, &fixture_scope(&service), &host, HostPointerEvent::Release { at: start });
        assert_eq!(released.candidate.is_some(), !leave, "an interrupted press must not become tab-click activation");
    }
}

#[test]
fn allocation_below_extension_stops_at_actual_presented_chrome_and_ignores_offscreen_bounds() {
    for direction in [Direction::Ltr, Direction::Rtl] {
        for zone in [Zone::Left, Zone::Right] {
            let mut service = fixture_service(false).unwrap();
            apply_fixture(
                &mut service,
                vec![
                    fixture_open("fixture.log", "target", "target", zone, 0).unwrap(),
                    Mutation::Open {
                        kind: PanelKind::new("fixture.log").unwrap(),
                        instance: id("moving"),
                        destination: Some(Destination::NewSection {
                            section: section("moving"),
                            index: 1,
                            location: SectionLocation::Floating { origin: Point { x: 600.0, y: 60.0 } },
                            size: Size { width: 280.0, height: 240.0 },
                        }),
                    },
                ],
            );
            let base = allocate_fixture(&service, Size { width: 1400.0, height: 900.0 }, direction, &fixture_metric_policy(&service));
            let card = base.sections.iter().find(|card| card.section == section("target")).unwrap();
            let start = card.outer.origin.y + card.outer.size.height;
            for (x, y, expected) in [
                (card.outer.origin.x, start + 160.0, start + 160.0),
                (card.outer.origin.x, start - 20.0, start),
                (card.outer.origin.x, 1000.0, 930.0),
                (600.0, start + 160.0, 930.0),
            ] {
                let chrome = LogicalRect { origin: Point { x, y }, size: Size { width: 80.0, height: 40.0 } };
                let host = resolve_host_layout_in_domain(
                    &service,
                    base.render_domain,
                    base.capacity,
                    &[chrome],
                    direction,
                    base.metrics,
                    &base.geometry_policy,
                )
                .unwrap();
                let targets = host_drop_targets(&service, &host, &id("moving")).unwrap();
                let below = targets
                    .iter()
                    .find(|target| target.id == HostDropTargetId::Relative { section: section("target"), placement: RelativePlacement::Below })
                    .unwrap();
                assert_eq!(below.bounds.origin.y + below.bounds.size.height, expected);
                let above = targets
                    .iter()
                    .find(|target| target.id == HostDropTargetId::Relative { section: section("target"), placement: RelativePlacement::Above })
                    .unwrap();
                assert_eq!(above.bounds.size.height, 56.0_f32.min(card.outer.size.height * 0.25));
            }
        }
    }
}

#[test]
fn allocation_below_extension_respects_presented_stack_and_collapsed_headers_but_skips_unavailable_members() {
    for direction in [Direction::Ltr, Direction::Rtl] {
        for zone in [Zone::Left, Zone::Right] {
            for state in ["expanded", "collapsed", "unavailable"] {
                let mut service = allocation_service(|spec| {
                    spec.max_size.height = 240.0;
                });
                apply_fixture(
                    &mut service,
                    vec![
                        fixture_open("fixture.log", "target", "target", zone, 0).unwrap(),
                        fixture_open("fixture.log", "obstacle", "obstacle", zone, 1).unwrap(),
                        Mutation::Open {
                            kind: PanelKind::new("fixture.log").unwrap(),
                            instance: id("moving"),
                            destination: Some(Destination::NewSection {
                                section: section("moving"),
                                index: 2,
                                location: SectionLocation::Floating { origin: Point { x: 600.0, y: 60.0 } },
                                size: Size { width: 280.0, height: 240.0 },
                            }),
                        },
                    ],
                );
                if state == "collapsed" {
                    apply_fixture(&mut service, vec![Mutation::Collapse { section: section("obstacle"), collapsed: true }]);
                } else if state == "unavailable" {
                    service.set_availability([(id("obstacle"), Availability::Unavailable { reason: "fixture hidden".into() })].into()).unwrap();
                }
                let host = allocate_fixture(&service, Size { width: 1400.0, height: 900.0 }, direction, &fixture_metric_policy(&service));
                let targets = host_drop_targets(&service, &host, &id("moving")).unwrap();
                let below = targets
                    .iter()
                    .find(|entry| entry.id == HostDropTargetId::Relative { section: section("target"), placement: RelativePlacement::Below })
                    .unwrap();
                let end = below.bounds.origin.y + below.bounds.size.height;
                if state == "unavailable" {
                    assert!(!host.sections.iter().any(|card| card.section == section("obstacle")));
                    assert_eq!(end, host.capacity.origin.y + host.capacity.size.height);
                } else {
                    let obstacle = host.sections.iter().find(|card| card.section == section("obstacle")).unwrap();
                    assert!(end <= obstacle.outer.origin.y, "a presented header/body or its sash bounds the clear corridor");
                }
            }
        }
    }
}

#[test]
fn allocation_capture_cancels_on_escape_cancel_scope_revision_availability_and_every_environment_change() {
    for case in 0..13 {
        let mut service = fixture_service(false).unwrap();
        apply_fixture(
            &mut service,
            vec![
                fixture_open("fixture.log", "target", "target", Zone::Left, 0).unwrap(),
                fixture_open("fixture.log", "moving", "moving", Zone::Floating, 1).unwrap(),
            ],
        );
        let host = allocate_fixture(&service, Size { width: 1400.0, height: 900.0 }, Direction::Ltr, &fixture_metric_policy(&service));
        let mut interaction = HostInteraction::default();
        fixture_press(&service, &host, &mut interaction, HostPointerTarget::Header { instance: id("moving") });
        let at = middle(host.sections.iter().find(|card| card.instance == id("target")).unwrap().outer);
        assert!(fixture_move(&service, &host, &mut interaction, at).rejection.is_none());
        let mut scope = fixture_scope(&service);
        let mut changed = host.clone();
        let event = match case {
            0 => HostPointerEvent::Escape,
            1 => HostPointerEvent::Cancel, // blur, hidden surface and modal adapters use this same route
            2 => {
                scope.viewport = ViewportId::new("other.viewport").unwrap();
                HostPointerEvent::Reconcile
            }
            3 => {
                apply_fixture(&mut service, vec![Mutation::Focus { instance: id("target") }]);
                HostPointerEvent::Reconcile
            }
            4 => {
                service.set_availability([(id("moving"), Availability::Unavailable { reason: "unavailable".into() })].into()).unwrap();
                HostPointerEvent::Reconcile
            }
            5 => {
                changed.geometry_policy.content_revision += 1;
                changed.allocation_environment.content_revision += 1;
                HostPointerEvent::Reconcile
            }
            6 => {
                changed.metrics.header_height += 1.0;
                HostPointerEvent::Reconcile
            }
            7 => {
                changed.direction = Direction::Rtl;
                HostPointerEvent::Reconcile
            }
            8 => {
                changed.capacity.size.width -= 1.0;
                HostPointerEvent::Reconcile
            }
            9 => {
                changed.geometry_policy.scroll.x = 10.0;
                HostPointerEvent::Reconcile
            }
            10 => HostPointerEvent::OutsidePress { at, inside_owned_overlay: false },
            11 => {
                scope.document = Some(DocumentId::new("other.document").unwrap());
                HostPointerEvent::Reconcile
            }
            _ => {
                changed.metrics.gap += 1.0;
                changed.allocation_environment.gap += 1.0;
                HostPointerEvent::Reconcile
            }
        };
        let current = service.inspect();
        let result = interaction.update(&service, &scope, &changed, event);
        assert!(result.candidate.is_none(), "case {case}");
        assert!(!interaction.is_captured() && interaction.preview().is_none(), "case {case}");
        assert!(interaction.move_source().is_none() && interaction.move_position().is_none(), "case {case}");
        assert_eq!(service.inspect(), current);
        assert!(interaction.update(&service, &fixture_scope(&service), &host, HostPointerEvent::Release { at }).candidate.is_none());
    }
}

#[test]
fn allocation_programmatic_admission_refuses_neutral_overflow_but_allows_strict_improvement_and_exemptions() {
    let mut service = fixture_service(false).unwrap();
    apply_fixture(
        &mut service,
        vec![fixture_open("fixture.log", "one", "one", Zone::Left, 0).unwrap(), fixture_open("fixture.log", "two", "two", Zone::Right, 1).unwrap()],
    );
    let host = allocate_fixture(&service, Size { width: 100.0, height: 90.0 }, Direction::Ltr, &fixture_metric_policy(&service));
    let scope = fixture_scope(&service);
    let before = service.inspect();
    assert_eq!(
        prepare_host_layout(
            &service,
            &scope,
            &host,
            transaction(vec![Mutation::SetSize { section: section("one"), size: service.layout().sections[0].size }])
        )
        .unwrap_err()
        .rule,
        Rule::NoSpace
    );
    for mutation in [
        Mutation::Close { instance: id("two") },
        Mutation::Collapse { section: section("one"), collapsed: true },
        Mutation::Collapse { section: section("one"), collapsed: false },
        Mutation::Focus { instance: id("one") },
    ] {
        assert!(prepare_host_layout(&service, &scope, &host, transaction(vec![mutation])).is_ok());
    }
    let improved = prepare_host_layout(
        &service,
        &scope,
        &host,
        transaction(vec![Mutation::Move { instance: id("two"), destination: Destination::JoinTab { section: section("one"), index: 1 } }]),
    )
    .unwrap();
    assert!(improved.layout().extent.width < host.extent.width);
    assert!(improved.layout().extent.height <= host.extent.height);
    assert_eq!(service.inspect(), before);
}

#[test]
fn allocation_pin_outer_follows_sides_and_invalid_selected_drop_never_displaces_a_peer() {
    let mut service = fixture_service(false).unwrap();
    apply_fixture(
        &mut service,
        vec![
            fixture_open("fixture.pin", "pin", "pin", Zone::Left, 0).unwrap(),
            fixture_open("fixture.log", "moving", "moving", Zone::Floating, 1).unwrap(),
        ],
    );
    let host = allocate_fixture(&service, Size { width: 1400.0, height: 900.0 }, Direction::Ltr, &fixture_metric_policy(&service));
    let before = service.inspect();
    let card = host.sections.iter().find(|card| card.instance == id("pin")).unwrap();
    let at = Point { x: card.outer.origin.x + 2.0, y: middle(card.outer).y };
    let mut interaction = HostInteraction::default();
    fixture_press(&service, &host, &mut interaction, HostPointerTarget::Header { instance: id("moving") });
    let preview = fixture_move(&service, &host, &mut interaction, at);
    assert!(preview.rejection.as_ref().is_some_and(|reason| matches!(reason.rule, Rule::PinOrder | Rule::PinConflict)));
    assert!(preview.projected.is_none() && preview.targets.is_empty());
    assert!(interaction.update(&service, &fixture_scope(&service), &host, HostPointerEvent::Release { at }).candidate.is_none());
    assert_eq!(service.inspect(), before);
    let candidate = prepare_host_layout(
        &service,
        &fixture_scope(&service),
        &host,
        transaction(vec![Mutation::Move {
            instance: id("pin"),
            destination: Destination::NewSection {
                section: section("pin"),
                index: 0,
                location: SectionLocation::Dock { zone: Zone::Right },
                size: Size { width: 280.0, height: 240.0 },
            },
        }]),
    )
    .unwrap();
    fixture_commit(&mut service, &host, candidate);
    assert_eq!(service.layout().containing(&id("pin")).unwrap().location.zone(), Zone::Right);
}

#[test]
fn allocation_paired_resize_and_double_click_transfer_owned_exact_candidates_with_composite_limits() {
    for direction in [Direction::Ltr, Direction::Rtl] {
        for zone in [Zone::Left, Zone::Right, Zone::Bottom] {
            let mut service = fixture_service(false).unwrap();
            apply_fixture(
                &mut service,
                vec![fixture_open("fixture.log", "one", "one", zone, 0).unwrap(), fixture_open("fixture.log", "two", "two", zone, 1).unwrap()],
            );
            let host = allocate_fixture(&service, Size { width: 1000.0, height: 800.0 }, direction, &fixture_metric_policy(&service));
            let before = service.inspect();
            let target = HostPointerTarget::Sash { first: section("one"), second: section("two") };
            let mut interaction = HostInteraction::default();
            let start = fixture_press(&service, &host, &mut interaction, target.clone());
            let at = if zone == Zone::Bottom {
                Point { x: start.x + if direction == Direction::Ltr { 20.0 } else { -20.0 }, y: start.y }
            } else {
                Point { x: start.x, y: start.y + 20.0 }
            };
            let preview = fixture_move(&service, &host, &mut interaction, at);
            assert!(preview.rejection.is_none(), "{zone:?}/{direction:?}: {preview:?}");
            assert!(preview.targets.is_empty());
            assert_eq!(service.inspect(), before);
            let release = interaction.update(&service, &fixture_scope(&service), &host, HostPointerEvent::Release { at });
            let committed = fixture_commit(&mut service, &host, release.candidate.unwrap());
            assert_eq!(committed.sashes[0].bounds, preview.bounds);
            let axis = |size: Size| if zone == Zone::Bottom { size.width } else { size.height };
            assert_eq!(
                service.layout().sections.iter().map(|section| axis(section.size)).sum::<f32>(),
                host.sections.iter().map(|section| axis(section.outer.size)).sum::<f32>()
            );
            let reset = interaction.update(&service, &fixture_scope(&service), &committed, HostPointerEvent::DoubleClick { target });
            assert!(reset.action.is_none());
            let restored = fixture_commit(&mut service, &committed, reset.candidate.unwrap());
            assert_eq!(restored.sashes[0].bounds, host.sashes[0].bounds);
        }
    }
}

#[test]
fn allocation_full_domain_and_short_wide_inline_roles_keep_real_usable_contours_and_header_order() {
    let mut service = allocation_service(|spec| {
        spec.min_size.height = 40.0;
        if spec.kind.0 == "fixture.log" {
            spec.header_actions =
                vec![HeaderActionSpec { id: ControlId::new("fixture.action").unwrap(), label_key: "ui.panel".into(), icon: "list".into() }];
            spec.features.hot_closable = true;
        }
    });
    apply_fixture(
        &mut service,
        vec![
            fixture_open("fixture.log", "one", "one", Zone::Bottom, 0).unwrap(),
            Mutation::SetSize { section: section("one"), size: Size { width: 800.0, height: 64.0 } },
        ],
    );
    let domain = LogicalRect { origin: Point { x: 0.0, y: 30.0 }, size: Size { width: 1400.0, height: 870.0 } };
    let capacity = LogicalRect { origin: Point { x: 0.0, y: 130.0 }, size: Size { width: 1400.0, height: 770.0 } };
    let ribbon = LogicalRect { origin: domain.origin, size: Size { width: 1400.0, height: 100.0 } };
    for direction in [Direction::Ltr, Direction::Rtl] {
        let host = resolve_host_layout_in_domain(
            &service,
            domain,
            capacity,
            &[ribbon],
            direction,
            HostMetrics { inline_max_height: 88.0, header_height: 20.0, footer_height: 16.0, ..HostMetrics::default() },
            &fixture_metric_policy(&service),
        )
        .unwrap();
        assert_eq!(host.viewport, Some(domain));
        assert!(host.usable.obstacles.contains(&ribbon));
        let card = &host.sections[0];
        assert!(card.inline && card.body.is_none() && card.footer.is_none());
        assert_eq!(card.header, card.outer, "one total row is shared by controls and inline content");
        let controls = [card.actions[0].1, card.menu, card.collapse.unwrap(), card.close.unwrap()];
        for pair in controls.windows(2) {
            assert!(if direction == Direction::Ltr { pair[0].origin.x < pair[1].origin.x } else { pair[0].origin.x > pair[1].origin.x });
        }
        assert!(host.usable.contains_rect(LogicalRect { origin: Point { x: 500.0, y: 300.0 }, size: Size { width: 40.0, height: 40.0 } }));
    }
}

#[test]
fn allocation_fill_collapse_is_header_only_and_expansion_uses_the_same_captured_policy() {
    let mut service = allocation_service(|spec| {
        spec.side_height_policy = SideHeightPolicy::Fill;
    });
    apply_fixture(&mut service, vec![fixture_open("fixture.log", "one", "one", Zone::Right, 0).unwrap()]);
    let host = allocate_fixture(&service, Size { width: 1200.0, height: 800.0 }, Direction::Rtl, &fixture_metric_policy(&service));
    assert_eq!(host.sections[0].outer.size.height, 800.0);
    let weight = service.layout().sections[0].weight;
    let collapse = prepare_host_layout(
        &service,
        &fixture_scope(&service),
        &host,
        transaction(vec![Mutation::Collapse { section: section("one"), collapsed: true }]),
    )
    .unwrap();
    let collapsed = fixture_commit(&mut service, &host, collapse);
    assert_eq!(collapsed.sections[0].outer.size.height, collapsed.metrics.header_height);
    assert!(collapsed.sections[0].body.is_none() && collapsed.sections[0].footer.is_none());
    assert!(collapsed.allocation_environment.content.is_empty(), "collapsed Fill never requests hidden body metrics");
    let expand = prepare_host_layout(
        &service,
        &fixture_scope(&service),
        &collapsed,
        transaction(vec![Mutation::Collapse { section: section("one"), collapsed: false }]),
    )
    .unwrap();
    let expanded = fixture_commit(&mut service, &collapsed, expand);
    assert_eq!(expanded.sections, host.sections);
    assert_eq!(service.layout().sections[0].weight, weight);
}

#[test]
fn allocation_floating_resize_retains_the_fitted_visible_anchor_and_cancels_outside_release() {
    let mut service = fixture_service(false).unwrap();
    apply_fixture(
        &mut service,
        vec![
            fixture_open("fixture.log", "one", "one", Zone::Floating, 0).unwrap(),
            Mutation::SetFloatingRect { section: section("one"), origin: Point { x: 700.0, y: 500.0 }, size: Size { width: 1000.0, height: 1000.0 } },
        ],
    );
    let host = allocate_fixture(&service, Size { width: 800.0, height: 600.0 }, Direction::Ltr, &fixture_metric_policy(&service));
    assert_eq!(host.sections[0].outer, host.capacity);
    let before = service.inspect();
    let target = HostPointerTarget::Resize { section: section("one"), edge: HostResizeEdge::BottomRight };
    let mut interaction = HostInteraction::default();
    let start = fixture_press(&service, &host, &mut interaction, target.clone());
    let at = Point { x: start.x - 40.0, y: start.y - 30.0 };
    let preview = fixture_move(&service, &host, &mut interaction, at);
    assert!(preview.rejection.is_none(), "{preview:?}");
    assert_eq!(preview.bounds.origin, host.capacity.origin);
    assert_eq!(preview.bounds.size, Size { width: 760.0, height: 570.0 });
    let release = interaction.update(&service, &fixture_scope(&service), &host, HostPointerEvent::Release { at });
    fixture_commit(&mut service, &host, release.candidate.unwrap());
    assert_eq!(service.layout().sections[0].location, SectionLocation::Floating { origin: host.capacity.origin });
    let current = allocate_fixture(&service, host.capacity.size, Direction::Ltr, &fixture_metric_policy(&service));
    let start = fixture_press(&service, &current, &mut interaction, target);
    let at = Point { x: start.x - 10.0, y: start.y - 10.0 };
    fixture_move(&service, &current, &mut interaction, at);
    let revision = service.inspect().revision;
    let result = interaction.update(&service, &fixture_scope(&service), &current, HostPointerEvent::Release { at: Point { x: 900.0, y: 900.0 } });
    assert!(result.candidate.is_none());
    assert_eq!(service.inspect().revision, revision);
    assert_eq!(revision, before.revision + 1);
}

#[test]
fn allocation_section_cap_refuses_split_but_lone_section_reparent_owns_reused_identity() {
    let mut service = fixture_service(false).unwrap();
    for index in 0..MAX_SECTIONS {
        apply_fixture(&mut service, vec![fixture_open("fixture.log", &format!("p{index}"), &format!("s{index}"), Zone::Left, index).unwrap()]);
    }
    apply_fixture(
        &mut service,
        vec![Mutation::Open {
            kind: PanelKind::new("fixture.log").unwrap(),
            instance: id("extra"),
            destination: Some(Destination::JoinTab { section: section("s0"), index: 1 }),
        }],
    );
    let host = allocate_fixture(&service, Size { width: 1400.0, height: 20000.0 }, Direction::Ltr, &fixture_metric_policy(&service));
    let before = service.inspect();
    let mut interaction = HostInteraction::default();
    fixture_press(&service, &host, &mut interaction, HostPointerTarget::Tab { instance: id("extra") });
    let at = Point { x: 1399.0, y: 500.0 };
    let preview = fixture_move(&service, &host, &mut interaction, at);
    assert_eq!(preview.rejection.unwrap().rule, Rule::CollectionLimit);
    assert!(interaction.update(&service, &fixture_scope(&service), &host, HostPointerEvent::Release { at }).candidate.is_none());
    assert_eq!(service.inspect(), before);
    fixture_press(&service, &host, &mut interaction, HostPointerTarget::Header { instance: id("p1") });
    let preview = fixture_move(&service, &host, &mut interaction, at);
    assert!(preview.rejection.is_none(), "{preview:?}");
    let release = interaction.update(&service, &fixture_scope(&service), &host, HostPointerEvent::Release { at });
    fixture_commit(&mut service, &host, release.candidate.unwrap());
    assert_eq!(service.layout().sections.len(), MAX_SECTIONS);
    assert_eq!(service.layout().containing(&id("p1")).unwrap().id, section("s1"));
    assert_eq!(service.layout().containing(&id("p1")).unwrap().location.zone(), Zone::Right);
}

#[test]
fn allocation_independent_refused_command_discards_pointer_candidate_without_any_other_mutation() {
    let mut service = fixture_service(false).unwrap();
    apply_fixture(
        &mut service,
        vec![
            fixture_open("fixture.log", "one", "one", Zone::Left, 0).unwrap(),
            fixture_open("fixture.log", "moving", "moving", Zone::Floating, 1).unwrap(),
        ],
    );
    let host = allocate_fixture(&service, Size { width: 1400.0, height: 900.0 }, Direction::Ltr, &fixture_metric_policy(&service));
    let before = service.inspect();
    let mut interaction = HostInteraction::default();
    fixture_press(&service, &host, &mut interaction, HostPointerTarget::Header { instance: id("moving") });
    let at = middle(host.sections[0].outer);
    assert!(fixture_move(&service, &host, &mut interaction, at).rejection.is_none());
    let result = interaction.prepare_command(
        &service,
        &fixture_scope(&service),
        &host,
        transaction(vec![Mutation::SetSize { section: section("one"), size: Size { width: -1.0, height: 240.0 } }]),
    );
    assert!(result.is_err());
    assert!(!interaction.is_captured() && interaction.preview().is_none());
    assert!(interaction.move_source().is_none() && interaction.move_position().is_none());
    assert!(interaction.update(&service, &fixture_scope(&service), &host, HostPointerEvent::Release { at }).candidate.is_none());
    assert_eq!(service.inspect(), before);
}

fn host_scope(h: &PanelMountHarness) -> LocalScope {
    let environment = h.runtime.environment();
    LocalScope { workspace: environment.workspace.clone(), document: environment.document.clone(), viewport: environment.viewport.clone() }
}

fn middle(rect: LogicalRect) -> Point {
    Point { x: rect.origin.x + rect.size.width * 0.5, y: rect.origin.y + rect.size.height * 0.5 }
}

fn host_logs(count: usize, zone: Zone) -> PanelMountHarness {
    let mut h = PanelMountHarness::new(false).unwrap();
    for index in 0..count {
        h.dispatch(vec![fixture_open("fixture.log", &format!("log-{index}"), &format!("section-{index}"), zone, index).unwrap()]).unwrap();
    }
    h.tick().unwrap();
    h
}

fn assert_host_geometry(h: &PanelMountHarness, host: &HostLayout) {
    host.resolved.validate().unwrap();
    for card in &host.sections {
        let slots: Vec<_> = card
            .title
            .into_iter()
            .chain(card.tabs.iter().map(|tab| tab.bounds))
            .chain(card.tab_overflow)
            .chain(card.actions.iter().map(|(_, bounds)| *bounds))
            .chain([card.menu, card.collapse.unwrap()])
            .chain(card.close)
            .chain(card.inline_content)
            .collect();
        for (index, bounds) in slots.iter().enumerate() {
            assert!(card.header.contains_rect(*bounds) || card.action_bar.is_some_and(|row| row.contains_rect(*bounds)));
            for other in slots.iter().skip(index + 1) {
                assert!(!egui_rect(*bounds).intersect(egui_rect(*other)).is_positive(), "disjoint header targets");
            }
        }
        if let Some(body) = card.body {
            assert_eq!(body.origin.y, bottom(card.action_bar.unwrap_or(card.header)) + host.metrics.content_header_height);
        }
    }
    let mut represented = std::collections::BTreeSet::new();
    for panel in &host.resolved.panels {
        assert!(panel.body.is_none_or(|body| panel.outer.contains_rect(body)));
        let section = h.runtime.layout().layout().section(&panel.section).unwrap();
        let (min, max) = tab_bounds(h.runtime.layout().layout(), h.runtime.layout().registry(), section).unwrap();
        assert!(panel.outer.size.width >= min.width && panel.outer.size.width <= max.width);
        if !section.collapsed {
            assert!(panel.outer.size.height >= min.height && panel.outer.size.height <= max.height);
        } else {
            assert_eq!(panel.outer.size.height, host.metrics.header_height);
        }
        assert!(represented.insert(panel.section.clone()));
    }
    for unpresented in &host.resolved.unpresented {
        assert!(represented.insert(unpresented.section.clone()));
    }
    assert_eq!(represented.len(), h.runtime.layout().layout().sections.len(), "every section is presented or truthfully unpresented");
    for control in &host.controls {
        control.bounds.validate().unwrap();
        assert!(host.capacity.contains_rect(control.bounds), "reachable control: {control:?}");
        assert!(!control.accessible_name.is_empty());
        assert!(host.painted.iter().any(|paint| paint.contains_rect(control.bounds)), "input must remain within actual paint");
    }
    if let Some(viewport) = host.viewport {
        viewport.validate().unwrap();
        assert_eq!(viewport, host.render_domain);
        assert!(host.render_domain.contains_rect(host.capacity));
    }
}

fn host_press(h: &PanelMountHarness, layout: &HostLayout, interaction: &mut HostInteraction, target: HostPointerTarget) -> Point {
    let at = middle(layout.control(&target).expect("source chrome must exist").bounds);
    let update = interaction.update(h.runtime.layout(), &host_scope(h), layout, HostPointerEvent::Press { at, target: Some(target) });
    assert!(update.action.is_none());
    assert!(update.preview.is_none());
    at
}

fn one_mutation(action: &LayoutAction) -> &Mutation {
    match action {
        LayoutAction::Apply { expected_revision: Some(_), mutations } => {
            assert_eq!(mutations.len(), 1, "one gesture is one atomic typed mutation");
            mutations.first().unwrap()
        }
        other => panic!("gesture must carry an optimistic Apply: {other:?}"),
    }
}

fn explicit_drop_point(h: &PanelMountHarness, layout: &HostLayout, instance: &InstanceId, target: &HostDropTargetId) -> Point {
    let targets = host_drop_targets(h.runtime.layout(), layout, instance).unwrap();
    middle(
        targets.iter().rev().find(|candidate| candidate.id == *target).unwrap_or_else(|| panic!("explicit drop target missing: {target:?}")).bounds,
    )
}

fn assert_join_drag_action(action: &LayoutAction, instance: InstanceId, target: SectionId, index: usize) {
    let LayoutAction::Apply { expected_revision: Some(_), mutations } = action else {
        panic!("a shown tab landing is one optimistic transaction");
    };
    assert_eq!(
        mutations,
        &vec![
            Mutation::Move { instance: instance.clone(), destination: Destination::JoinTab { section: target, index } },
            Mutation::Activate { instance }
        ],
        "the dragged tab is visible at the previewed insertion point"
    );
}

fn host_with_specs(edit: impl Fn(&mut PanelSpec)) -> PanelMountHarness {
    let mut h = PanelMountHarness::new(false).unwrap();
    let mut registry = PanelRegistry::default();
    for spec in synthetic_registry().unwrap().specs() {
        let mut spec = spec.clone();
        edit(&mut spec);
        registry.register(spec).unwrap();
    }
    let environment = h.runtime.environment().clone();
    h.runtime = PanelRuntime::new(
        LayoutService::new(registry, Layout::empty(environment.workspace.clone()), HostCapabilities::default()).unwrap(),
        environment,
    )
    .unwrap();
    bind_synthetic_controllers(&mut h.runtime, &h.probe).unwrap();
    h
}

#[test]
fn usable_region_preserves_short_panel_gaps_floating_holes_and_every_late_obstacle() {
    let domain = LogicalRect { origin: Point { x: 0.0, y: 0.0 }, size: Size { width: 1000.0, height: 600.0 } };
    let obstacles = [
        LogicalRect { origin: Point { x: 0.0, y: 0.0 }, size: Size { width: 200.0, height: 120.0 } },
        LogicalRect { origin: Point { x: 800.0, y: 0.0 }, size: Size { width: 200.0, height: 240.0 } },
        LogicalRect { origin: Point { x: 450.0, y: 200.0 }, size: Size { width: 100.0, height: 100.0 } },
    ];
    let usable = resolve_usable_region(domain, &obstacles).unwrap();
    assert_eq!(usable.free.iter().map(|rect| rect.size.width * rect.size.height).sum::<f32>(), 518000.0);
    for (index, piece) in usable.free.iter().enumerate() {
        assert!(usable.contains_rect(*piece));
        assert!(usable.free.iter().skip(index + 1).all(|other| !egui_rect(*piece).intersect(egui_rect(*other)).is_positive()));
    }
    assert!(usable.free.iter().any(|rect| rect.contains(Point { x: 100.0, y: 400.0 })), "space below a short left panel remains usable");
    assert!(usable.free.iter().any(|rect| rect.contains(Point { x: 900.0, y: 400.0 })), "asymmetric right-panel height cannot discard the lower gap");
    assert!(!usable.free.iter().any(|rect| rect.contains(Point { x: 500.0, y: 250.0 })), "floating footprints remain holes");
    let footprint = usable.place_footprint(Size { width: 80.0, height: 40.0 }, Point { x: 100.0, y: 400.0 }, 10.0).unwrap();
    assert_eq!(footprint, LogicalRect { origin: Point { x: 60.0, y: 380.0 }, size: Size { width: 80.0, height: 40.0 } });
    assert!(usable.contains_rect(LogicalRect { origin: Point { x: 50.0, y: 370.0 }, size: Size { width: 100.0, height: 60.0 } }));
    let fit = usable.fit_rect(2.0, Point { x: 500.0, y: 300.0 }).unwrap();
    assert!(usable.contains_rect(fit));
    assert!(
        (fit.size.width - 600.0).abs() < 0.01 && (fit.size.height - 300.0).abs() < 0.01,
        "model aspect must use the broad region below the floating hole: {fit:?}"
    );
    assert!(usable.place_footprint(Size { width: 1001.0, height: 601.0 }, middle(domain), 0.0).is_none());
    assert_eq!(usable.fit_rect_with_clearance(2.0, middle(domain), 0.0), Some(fit));
    for aspect in [100.0, 0.01] {
        let inner = usable.fit_rect_with_clearance(aspect, middle(domain), 8.0).expect("clearance must be reserved without erasing a thin fit");
        assert!((inner.size.width / inner.size.height - aspect).abs() < aspect * 0.0001);
        let padded = egui_rect(inner).expand(8.0);
        assert!(usable.contains_rect(logical_rect(padded)));
        assert!(obstacles.iter().all(|obstacle| !padded.intersect(egui_rect(*obstacle)).is_positive()));
        if aspect > 1.0 {
            assert!(inner.size.width > 900.0 && inner.size.height < 16.0);
        } else {
            assert!(inner.size.height > 500.0 && inner.size.width < 16.0);
        }
    }
    for clearance in [f32::NAN, -1.0, f32::INFINITY, f32::MAX, 600.0] {
        assert!(usable.fit_rect_with_clearance(1.0, middle(domain), clearance).is_none());
    }
    for aspect in [f32::NAN, -1.0, 0.0, f32::INFINITY] {
        assert!(usable.fit_rect_with_clearance(aspect, middle(domain), 8.0).is_none());
    }
    let tiny_domain = LogicalRect { origin: Point { x: 25.0, y: 35.0 }, size: Size { width: 8.0, height: 8.0 } };
    let tiny = resolve_usable_region(tiny_domain, &[]).unwrap();
    assert!(tiny.fit_rect_with_clearance(1.0, middle(tiny_domain), 4.0).is_none());
    assert!(tiny.fit_rect_with_clearance(1.0, middle(tiny_domain), 1.0).is_some());

    let mut many = (0..20)
        .map(|index| LogicalRect { origin: Point { x: index as f32 * 40.0, y: 0.0 }, size: Size { width: 20.0, height: 20.0 } })
        .collect::<Vec<_>>();
    let late = LogicalRect { origin: Point { x: 460.0, y: 250.0 }, size: Size { width: 80.0, height: 100.0 } };
    many.push(late);
    let crowded = resolve_usable_region(domain, &many).unwrap();
    assert_eq!(crowded.obstacles.len(), 21);
    assert!(
        crowded.free.iter().all(|piece| !egui_rect(*piece).intersect(egui_rect(late)).is_positive()),
        "an obstacle after the old sixteen-entry bound must never be ignored"
    );
    let placed = crowded.place_footprint(Size { width: 60.0, height: 40.0 }, middle(late), 8.0).unwrap();
    assert!(!egui_rect(placed).expand(8.0).intersect(egui_rect(late)).is_positive());
    assert_eq!(crowded.place_footprint(Size { width: 60.0, height: 40.0 }, middle(late), 8.0), Some(placed), "placement ties are stable");
}

/// Drives the production workspace chrome with separate press/move/release
/// RawInput frames. This is headless UI coverage, not visual evidence.
#[derive(Clone, Debug)]
struct RawModalObservation {
    card: egui::Rect,
    backdrop: egui::Rect,
    visible: bool,
    escape_events: Vec<egui::Event>,
    close_requested: bool,
}

struct RawPanelHost {
    content: PanelMountHarness,
    host: WorkspacePanelHost,
    ctx: egui::Context,
    time: f64,
    capacity: LogicalRect,
    native_pixels_per_point: Option<f32>,
    foreign_cover: Option<egui::Rect>,
    foreign_responses: Vec<egui::Response>,
    foreign_takes_focus: bool,
    foreign_raise_late: bool,
    foreign_order: egui::Order,
    stale_foreign_cover: Option<egui::Rect>,
    stale_foreign_layer: Option<egui::LayerId>,
    root_layer: Option<egui::LayerId>,
    painted_frames: Vec<HostFrame>,
    finished_frames: Vec<HostFrame>,
    clicked_widgets: Vec<egui::Id>,
    double_clicked_widgets: Vec<egui::Id>,
    painted_menu: bool,
    late_modal: bool,
    reserve_escape_for_host: bool,
    escape_reservations: usize,
    modal_observations: Vec<RawModalObservation>,
    focused_response: Option<egui::Response>,
    bind_fixture_scopes: bool,
}

impl RawPanelHost {
    fn new(content: PanelMountHarness, scale: f32) -> Self {
        let ctx = crate::fonts::test_context();
        ctx.enable_accesskit();
        ctx.set_pixels_per_point(scale);
        let host = WorkspacePanelHost::new(HostOptions::default()).unwrap();
        Self {
            content,
            host,
            ctx,
            time: 0.0,
            capacity: capacity(),
            native_pixels_per_point: None,
            foreign_cover: None,
            foreign_responses: Vec::new(),
            foreign_takes_focus: false,
            foreign_raise_late: false,
            foreign_order: egui::Order::Foreground,
            stale_foreign_cover: None,
            stale_foreign_layer: None,
            root_layer: None,
            painted_frames: Vec::new(),
            finished_frames: Vec::new(),
            clicked_widgets: Vec::new(),
            double_clicked_widgets: Vec::new(),
            painted_menu: false,
            late_modal: false,
            reserve_escape_for_host: false,
            escape_reservations: 0,
            modal_observations: Vec::new(),
            focused_response: None,
            bind_fixture_scopes: false,
        }
    }

    fn frame(&mut self, events: Vec<egui::Event>, multipass: bool) -> (HostFrame, egui::FullOutput) {
        let (frame, output) = self.frame_late(events, multipass, |_, _| {});
        (frame.expect("ordinary host frame remains present"), output)
    }

    fn frame_late(
        &mut self,
        events: Vec<egui::Event>,
        multipass: bool,
        mut after_show: impl FnMut(&mut PanelMountHarness, &egui::Context),
    ) -> (Option<HostFrame>, egui::FullOutput) {
        self.content.tick().unwrap();
        self.time += 1.0 / 60.0;
        let mut passes = 0;
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(self.capacity.size.width, self.capacity.size.height))),
            events,
            time: Some(self.time),
            focused: true,
            ..Default::default()
        };
        if let Some(dpi) = self.native_pixels_per_point {
            input.viewports.entry(egui::ViewportId::ROOT).or_default().native_pixels_per_point = Some(dpi);
        }
        // Match SolveApp's native boundary before egui validates pointer and wheel events.
        crate::finite_input(&mut input);
        self.foreign_responses.clear();
        self.painted_frames.clear();
        self.finished_frames.clear();
        self.clicked_widgets.clear();
        self.double_clicked_widgets.clear();
        self.modal_observations.clear();
        self.focused_response = None;
        self.escape_reservations = 0;
        let mut output = self.ctx.run_ui(input, |ui| {
            passes += 1;
            self.host.capture_metrics(&self.content.runtime, &self.content.models).unwrap();
            self.host.capture_reservations(&self.content.runtime, &self.content.catalogue).unwrap();
            self.host.prepare(&self.content.runtime, self.capacity).unwrap();
            self.root_layer = Some(ui.layer_id());
            if self.bind_fixture_scopes {
                // The strict existing fake provider requires an explicit bind
                // before a controller can request an overlay. Rebind the live
                // owner scope for each real mount, including a post-commit pass.
                let scope = host_scope(&self.content);
                for instance in &self.content.runtime.layout().layout().instances {
                    self.content.local.bind(instance.id.clone(), scope.clone()).unwrap();
                }
            }
            if self.reserve_escape_for_host
                && self.host.wants_escape(&self.content.runtime)
                && crate::ui_kit::Container::reserve_host_escape(ui.ctx())
            {
                self.escape_reservations += 1;
                self.host.request_escape();
            }
            let h = &mut self.content;
            self.host
                .show_deferred(
                    ui,
                    &mut h.runtime,
                    &mut h.native,
                    &mut ContentServices { models: &h.models, commands: &mut h.commands, local: &mut h.local },
                )
                .unwrap();
            let painted = self.host.frame().unwrap().clone();
            for control in &painted.controls {
                if let Some(response) = ui.ctx().read_response(control.id) {
                    if response.clicked() || response.secondary_clicked() {
                        self.clicked_widgets.push(control.id);
                    }
                    if response.double_clicked() {
                        self.double_clicked_widgets.push(control.id);
                    }
                }
            }
            self.painted_frames.push(painted);
            // Match real late system UI: the host renders before this unrelated
            // foreground container. Warmed frames establish real egui ownership.
            if let Some(bounds) = self.foreign_cover {
                let container = crate::ui_kit::Container::new(ui.ctx(), crate::ui_kit::Surface::Window);
                let frame = container.frame(ui.ctx(), solidcraft_theme::Tokens::get().dialog_body, crate::frame::Side::Left).inner_margin(0);
                container
                    .area(egui::Id::unique("unrelated_test_window"))
                    .order(self.foreign_order)
                    .fixed_pos(bounds.min)
                    .movable(false)
                    .constrain(false)
                    .fade_in(false)
                    .show(ui.ctx(), |ui| {
                        ui.set_min_size(bounds.size());
                        container.show_frame(ui, frame, bounds, |ui| {
                            ui.set_min_size(bounds.size());
                            let response = ui.interact(bounds, ui.scope_id().with("foreign_control"), egui::Sense::click_and_drag());
                            response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Unrelated foreground control"));
                            if self.foreign_takes_focus {
                                response.request_focus();
                            }
                            self.foreign_responses.push(response);
                        });
                        let covered = bounds.intersect(ui.clip_rect()).intersect(egui_rect(self.capacity));
                        if ui.is_visible() && !ui.is_sizing_pass() && covered.is_positive() {
                            assert!(
                                ui.ctx().graphics(|graphics| {
                                    graphics.get(ui.layer_id()).is_some_and(|paint| {
                                        paint.all_entries().any(|entry| {
                                            let painted =
                                                (entry.transform_after_tessellation * entry.shape.visual_bounding_rect()).intersect(entry.clip_rect);
                                            painted.is_positive() && painted.is_finite() && painted.contains(covered.center())
                                        })
                                    })
                                }),
                                "the foreign frame must actually paint at the covered point"
                            );
                        }
                    });
                if self.foreign_raise_late {
                    let layer = self.foreign_responses.last().expect("actual foreign Area response").layer_id;
                    crate::ui_kit::Container::bring_to_front(ui.ctx(), layer);
                }
            }
            if let Some(bounds) = self.stale_foreign_cover {
                let container = crate::ui_kit::Container::new(ui.ctx(), crate::ui_kit::Surface::Window);
                container
                    .area(egui::Id::unique("stale_third_test_window"))
                    .order(egui::Order::Foreground)
                    .fixed_pos(bounds.min)
                    .constrain(false)
                    .movable(false)
                    .sense(egui::Sense::hover())
                    .fade_in(false)
                    .show(ui.ctx(), |ui| {
                        ui.set_min_size(bounds.size());
                        self.stale_foreign_layer = Some(ui.layer_id());
                        crate::ui_kit::Container::paint_frame(ui.painter(), crate::ui_kit::Container::menu_frame(), bounds);
                    });
            }
            let modal = self.late_modal.then(|| {
                let container = crate::ui_kit::Container::new(ui.ctx(), crate::ui_kit::Surface::Window);
                let frame = container.frame(ui.ctx(), solidcraft_theme::Tokens::get().dialog_body, crate::frame::Side::Left);
                container.modal(egui::Id::unique("unrelated_test_modal")).frame(frame).show(ui.ctx(), |ui| {
                    ui.set_min_size(egui::vec2(200.0, 80.0));
                    ui.label("Independent modal content");
                    (
                        ui.is_visible() && !ui.is_sizing_pass(),
                        ui.input(|input| {
                            input
                                .events
                                .iter()
                                .filter(|event| matches!(event, egui::Event::Key { key: egui::Key::Escape, pressed: true, .. }))
                                .cloned()
                                .collect::<Vec<_>>()
                        }),
                    )
                })
            });
            after_show(h, ui.ctx());
            self.host
                .finish(
                    ui,
                    &mut h.runtime,
                    &mut h.native,
                    &mut ContentServices { models: &h.models, commands: &mut h.commands, local: &mut h.local },
                    &mut h.catalogue,
                )
                .unwrap();
            if let Some(modal) = modal {
                // The modal receives Escape after host finish as it does in a
                // late system UI path; an earlier host must not consume it.
                let close_requested = modal.should_close();
                self.modal_observations.push(RawModalObservation {
                    card: modal.response.rect,
                    // Ui::response captured inside Modal::show can still carry
                    // the preceding pass's rectangle. The child has now dropped
                    // and registered its actual current-pass backdrop bounds.
                    backdrop: ui.ctx().read_response(modal.backdrop_response.id).unwrap().rect,
                    visible: modal.inner.0,
                    escape_events: modal.inner.1,
                    close_requested,
                });
            }
            if let Some(frame) = self.host.frame() {
                self.finished_frames.push(frame.clone());
            }
            self.focused_response = ui.memory(|memory| memory.focused()).and_then(|id| ui.ctx().read_response(id));
            if multipass && ui.ctx().current_pass_index() == 0 {
                ui.ctx().request_discard("exercise universal host one-shot actions across egui passes");
            }
        });
        // This headless fixture inspects shapes and has no texture painter.
        output.textures_delta.clear();
        if multipass {
            assert!(passes >= 2, "must actually exercise another egui pass");
        }
        self.painted_menu |= self.host.frame().is_some_and(|frame| frame.menu_bounds.is_some());
        (self.host.frame().cloned(), output)
    }

    fn at(&self, target: HostControlTarget) -> egui::Pos2 {
        let bounds = self
            .host
            .frame()
            .unwrap()
            .controls
            .iter()
            .find(|control| control.target == target)
            .unwrap_or_else(|| panic!("actual chrome control missing: {target:?}"))
            .bounds;
        let at = middle(bounds);
        egui::pos2(at.x, at.y)
    }

    fn pointer(&mut self, at: egui::Pos2, pressed: bool, multipass: bool) -> HostFrame {
        self.frame(
            vec![
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE },
            ],
            multipass,
        )
        .0
    }

    fn click(&mut self, target: HostControlTarget, multipass: bool) -> HostFrame {
        let at = self.at(target);
        self.frame(vec![egui::Event::PointerMoved(at)], false);
        self.pointer(at, true, multipass);
        self.pointer(at, false, multipass)
    }

    fn menu(&mut self, instance: InstanceId) {
        self.click(HostControlTarget::Pointer(HostPointerTarget::Menu { instance }), false);
        self.frame(Vec::new(), false);
        self.frame(Vec::new(), false);
        assert!(self.host.frame().unwrap().menu_bounds.is_some(), "must open the real shared menu");
    }

    /// egui's first-ever Area pass is invisible measurement. Measure the
    /// unrelated window away from the viewport before its first visible late
    /// appearance, so covered-input cases exercise a real enabled foreign UI.
    fn prime_foreign_window(&mut self) {
        self.foreign_cover = Some(egui::Rect::from_min_size(
            egui::pos2(self.capacity.size.width + 200.0, self.capacity.size.height + 200.0),
            egui::vec2(180.0, 100.0),
        ));
        self.frame(Vec::new(), false);
        self.frame(Vec::new(), false);
        assert!(!self.foreign_responses.is_empty());
        assert!(self.foreign_responses.iter().all(egui::Response::enabled));
        self.foreign_cover = None;
        self.frame(Vec::new(), false);
    }

    fn prime_modal(&mut self) {
        self.late_modal = true;
        self.frame(Vec::new(), false);
        self.frame(Vec::new(), false);
        assert!(self.modal_observations.iter().all(|modal| modal.visible));
        self.late_modal = false;
        self.frame(Vec::new(), false);
        self.frame(Vec::new(), false);
        assert!(!crate::ui_kit::Container::modal_input_blocked(&self.ctx));
    }
}

#[test]
fn raw_host_tab_activation_renders_only_active_content_and_keeps_hidden_services_and_drafts() {
    for scale in [1.0, 2.0] {
        let mut h = PanelMountHarness::new(false).unwrap();
        open_editor(&mut h);
        h.dispatch(vec![Mutation::Open {
            kind: PanelKind::new("fixture.log").unwrap(),
            instance: id("inactive"),
            destination: Some(Destination::JoinTab { section: section("editor-section"), index: 1 }),
        }])
        .unwrap();
        let key = editor_key(&h);
        h.probe.edit(&key, |state| state.draft = "keep this edit".into()).unwrap();
        let mut raw = RawPanelHost::new(h, scale);
        raw.frame(Vec::new(), false);
        let inactive_key =
            ControllerKey { instance: id("inactive"), owner: ContentOwner::Workspace(raw.content.runtime.environment().workspace.clone()) };
        assert_eq!(raw.content.probe.state(&inactive_key).unwrap().renders, 0);
        let editor_renders = raw.content.probe.state(&key).unwrap().renders;
        raw.click(HostControlTarget::Pointer(HostPointerTarget::Tab { instance: id("inactive") }), false);
        raw.frame(Vec::new(), false);
        assert_eq!(raw.content.runtime.layout().layout().section(&section("editor-section")).unwrap().active, id("inactive"));
        assert!(raw.content.runtime.layout().interaction().focused.is_none(), "Activate does not manufacture keyboard focus");
        let editor_after_activation = raw.content.probe.state(&key).unwrap().renders;
        assert!(editor_after_activation >= editor_renders);
        let updates = raw.content.probe.state(&key).unwrap().updates;
        for _ in 0..3 {
            raw.frame(Vec::new(), false);
        }
        let state = raw.content.probe.state(&key).unwrap();
        assert_eq!(state.renders, editor_after_activation);
        assert!(state.updates > updates);
        assert_eq!(state.draft, "keep this edit");
        assert!(state.transaction_live);
        assert_eq!((state.cancels, state.applies, state.finishes), (0, 0, 0));
        assert!(raw.content.probe.state(&inactive_key).unwrap().renders > 0);
    }
}

#[test]
fn raw_host_content_actions_commit_once_across_real_egui_multipass_and_do_not_start_drags() {
    let mut h = PanelMountHarness::new(false).unwrap();
    open_editor(&mut h);
    let key = editor_key(&h);
    let mut raw = RawPanelHost::new(h, 2.0);
    raw.frame(Vec::new(), false);
    let before = raw.content.runtime.layout().inspect();
    let released = raw.click(HostControlTarget::Content { instance: id("editor"), action: ControlId::new("apply").unwrap() }, true);
    assert_eq!(
        released
            .events
            .iter()
            .filter(|event| matches!(event, HostEvent::ContentAction { instance, action, result: Ok(()) }
            if *instance == id("editor") && action.0 == "apply"))
            .count(),
        1,
        "the repeated release pass publishes the executed content outcome once"
    );
    assert_eq!(raw.content.commands.calls, 1);
    assert_eq!(raw.content.probe.state(&key).unwrap().applies, 1);
    assert_eq!(raw.content.runtime.layout().inspect(), before);
    assert!(!raw.host.interaction().is_captured());
    assert!(raw.host.frame().unwrap().preview.is_none());
    let next = raw.frame(Vec::new(), true).0;
    assert!(!next.events.iter().any(|event| matches!(event, HostEvent::ContentAction { .. })));
    assert_eq!(raw.content.commands.calls, 1, "later passes/frames cannot replay a content action");
}

#[test]
fn raw_host_explicit_close_refusal_keeps_layout_native_resources_and_transaction_atomic() {
    let mut h = PanelMountHarness::new(false).unwrap();
    open_editor(&mut h);
    let key = editor_key(&h);
    h.probe
        .edit(&key, |state| {
            state.draft = "guarded edit".into();
            state.reject_cancel = true;
        })
        .unwrap();
    let mut raw = RawPanelHost::new(h, 1.0);
    raw.frame(Vec::new(), false);
    raw.menu(id("editor"));
    let before = raw.content.runtime.layout().inspect();
    let native_commits = raw.content.native.commits;
    let controllers = raw.content.runtime.controller_count();
    let frame = raw.click(HostControlTarget::Shared { instance: id("editor"), action: HostSharedAction::Close }, true);
    assert_eq!(raw.content.runtime.layout().inspect(), before);
    assert_eq!(raw.content.native.commits, native_commits);
    assert_eq!(raw.content.runtime.controller_count(), controllers);
    assert!(frame.events.iter().any(|event| matches!(event, HostEvent::Rejected { error, .. } if error.rule == Rule::Lifecycle)));
    let state = raw.content.probe.state(&key).unwrap();
    assert!(state.transaction_live);
    assert_eq!(state.draft, "guarded edit");
    assert_eq!(state.cancels, 0);
    raw.content.probe.edit(&key, |state| state.reject_cancel = false).unwrap();
    // A refused action may leave the menu open. Open it only if needed.
    raw.frame(Vec::new(), false);
    if raw.host.frame().unwrap().menu_bounds.is_none() {
        raw.menu(id("editor"));
    }
    raw.click(HostControlTarget::Shared { instance: id("editor"), action: HostSharedAction::Close }, true);
    assert!(!raw.content.runtime.layout().layout().instance(&id("editor")).unwrap().requested_open);
    assert_eq!(raw.content.probe.state(&key).unwrap().cancels, 1);
    assert_eq!(raw.content.commands.calls, 0);
}

#[test]
fn raw_host_shared_move_menu_is_a_pointer_independent_alternative_to_drag() {
    let mut h = PanelMountHarness::new(false).unwrap();
    open_editor(&mut h);
    let key = editor_key(&h);
    h.probe.edit(&key, |state| state.draft = "menu move draft".into()).unwrap();
    let mut raw = RawPanelHost::new(h, 1.0);
    raw.frame(Vec::new(), false);
    for zone in [Zone::Right, Zone::Bottom, Zone::Floating, Zone::Left] {
        raw.menu(id("editor"));
        let before = raw.content.runtime.layout().inspect().revision;
        raw.click(HostControlTarget::Shared { instance: id("editor"), action: HostSharedAction::Move(zone) }, true);
        raw.frame(Vec::new(), false);
        assert_eq!(raw.content.runtime.layout().inspect().revision, before + 1);
        assert_eq!(raw.content.runtime.layout().layout().containing(&id("editor")).unwrap().location.zone(), zone);
        let state = raw.content.probe.state(&key).unwrap();
        assert_eq!(state.draft, "menu move draft");
        assert!(state.transaction_live);
        assert_eq!((state.applies, state.cancels, state.finishes), (0, 0, 0));
        assert_eq!(raw.content.commands.calls, 0);
    }
}

#[test]
fn raw_host_header_collapse_restore_and_hide_retain_the_draft_and_tick_hidden_services() {
    let mut h = PanelMountHarness::new(false).unwrap();
    open_editor(&mut h);
    let key = editor_key(&h);
    h.probe
        .edit(&key, |state| {
            state.draft = "live hidden edit".into();
            state.reject_cancel = true;
        })
        .unwrap();
    let mut raw = RawPanelHost::new(h, 2.0);
    raw.frame(Vec::new(), false);
    let target = HostControlTarget::Pointer(HostPointerTarget::Collapse { instance: id("editor") });
    raw.click(target.clone(), false);
    raw.frame(Vec::new(), false);
    let hidden = raw.content.probe.state(&key).unwrap();
    let card = raw.host.frame().unwrap().layout.sections.first().unwrap();
    assert!(card.collapsed && card.body.is_none() && card.footer.is_none());
    assert_eq!(card.outer, card.header);
    for _ in 0..3 {
        raw.frame(Vec::new(), false);
    }
    let idle = raw.content.probe.state(&key).unwrap();
    assert_eq!(idle.renders, hidden.renders);
    assert!(idle.updates > hidden.updates);
    raw.click(target, false);
    raw.frame(Vec::new(), false);
    assert!(!raw.content.runtime.layout().layout().section(&section("editor-section")).unwrap().collapsed);
    assert!(raw.content.probe.state(&key).unwrap().renders > idle.renders);
    let mut layout = raw.content.runtime.layout().layout().clone();
    layout.sections.clear();
    layout.instances[0].requested_open = false;
    raw.content.apply_layout(layout, None).unwrap();
    raw.frame(Vec::new(), false);
    let hidden_again = raw.content.probe.state(&key).unwrap().renders;
    for _ in 0..3 {
        raw.frame(Vec::new(), false);
    }
    let state = raw.content.probe.state(&key).unwrap();
    assert_eq!(state.renders, hidden_again);
    assert_eq!(state.draft, "live hidden edit");
    assert!(state.transaction_live);
    assert_eq!((state.cancels, state.applies, state.finishes, state.destroys), (0, 0, 0, 0));
    assert_eq!(raw.content.commands.calls, 0);
    assert!(raw.host.frame().unwrap().layout.resolved.panels.is_empty());
    assert!(!raw.content.runtime.state(&key).unwrap().presented);
}
#[test]
fn raw_host_native_prepare_failure_and_capability_false_menu_preserve_atomicity() {
    for native in [false, true] {
        let mut h = PanelMountHarness::new(native).unwrap();
        open_editor(&mut h);
        let key = editor_key(&h);
        let mut raw = RawPanelHost::new(h, 1.0);
        raw.frame(Vec::new(), false);
        raw.menu(id("editor"));
        let control = raw
            .host
            .frame()
            .unwrap()
            .controls
            .iter()
            .find(|control| control.target == HostControlTarget::Shared { instance: id("editor"), action: HostSharedAction::Popout });
        assert_eq!(control.is_some_and(|control| control.enabled), native, "only supported pop-outs appear");
        if !native {
            assert!(control.is_none());
            continue;
        }
        let before = raw.content.runtime.layout().inspect();
        let commits = raw.content.native.commits;
        raw.content.native.fail_prepare = true;
        let frame = raw.click(HostControlTarget::Shared { instance: id("editor"), action: HostSharedAction::Popout }, false);
        assert_eq!(raw.content.runtime.layout().inspect(), before);
        assert_eq!(raw.content.native.commits, commits);
        assert!(raw.content.native.hosts.is_empty());
        assert_eq!(raw.content.probe.state(&key).unwrap().reparents, 0);
        assert!(raw.content.probe.state(&key).unwrap().transaction_live);
        if native {
            assert!(frame.events.iter().any(|event| matches!(event, HostEvent::Rejected { .. })));
        }
    }
}

#[test]
fn raw_host_widget_ids_follow_semantic_instances_and_scope_not_title_or_position() {
    let mut h = host_logs(2, Zone::Left);
    h.tick().unwrap();
    let mut raw = RawPanelHost::new(h, 1.0);
    let target = HostControlTarget::Pointer(HostPointerTarget::Header { instance: id("log-0") });
    let (frame, _) = raw.frame(Vec::new(), false);
    let original = frame.controls.iter().find(|control| control.target == target).unwrap().id;
    raw.content
        .dispatch(vec![Mutation::Move {
            instance: id("log-0"),
            destination: Destination::NewSection {
                section: section("moved"),
                index: 1,
                location: SectionLocation::Dock { zone: Zone::Right },
                size: Size { width: 320.0, height: 280.0 },
            },
        }])
        .unwrap();
    let (moved, _) = raw.frame(Vec::new(), false);
    assert_eq!(moved.controls.iter().find(|control| control.target == target).unwrap().id, original);
    let mut environment = raw.content.runtime.environment().clone();
    environment.viewport = ViewportId::new("other-local-window").unwrap();
    environment.window = HostId::new("other-window").unwrap();
    raw.content.runtime.set_environment(environment, &mut raw.content.local).unwrap();
    let (other_window, _) = raw.frame(Vec::new(), false);
    assert_ne!(
        other_window.controls.iter().find(|control| control.target == target).unwrap().id,
        original,
        "native/window peers must not share a widget namespace"
    );
}

#[test]
fn raw_host_natural_edges_win_tab_borders_trailing_halves_and_corner_arbitration_at_both_scales() {
    for scale in [1.0, 2.0] {
        for direction in [Direction::Ltr, Direction::Rtl] {
            for zone in [Zone::Left, Zone::Right] {
                for case in ["tab-border", "corner", "column-inward", "outward", "outward-far", "trailing-half"] {
                    let mut h = host_logs(1, zone);
                    h.dispatch(vec![
                        Mutation::Open {
                            kind: PanelKind::new("fixture.log").unwrap(),
                            instance: id("tab-peer"),
                            destination: Some(Destination::JoinTab { section: section("section-0"), index: 1 }),
                        },
                        fixture_open("fixture.log", "moving", "moving", Zone::Floating, 1).unwrap(),
                        Mutation::SetFloatingRect {
                            section: section("moving"),
                            origin: Point { x: 560.0, y: 400.0 },
                            size: Size { width: 280.0, height: 240.0 },
                        },
                    ])
                    .unwrap();
                    let mut environment = h.runtime.environment().clone();
                    environment.direction = direction;
                    h.runtime.set_environment(environment, &mut h.local).unwrap();
                    let mut raw = RawPanelHost::new(h, scale);
                    let options = priority_host_options();
                    raw.host = WorkspacePanelHost::new(options).unwrap();
                    raw.frame(Vec::new(), false);
                    raw.frame(Vec::new(), false);
                    let initial = raw.host.frame().unwrap().layout.clone();
                    let card = initial.sections.iter().find(|card| card.section == section("section-0")).unwrap();
                    let outer_left = direction.edge(zone).unwrap() == PhysicalEdge::Left;
                    let outer_x = if outer_left { card.outer.origin.x + 45.0 } else { card.outer.origin.x + card.outer.size.width - 45.0 };
                    let tab = card.tabs.first().unwrap().bounds;
                    let (end, placement) = match case {
                        "tab-border" => (egui::pos2(outer_x, tab.origin.y + 1.0), RelativePlacement::Above),
                        "corner" => (egui::pos2(outer_x, card.outer.origin.y + 45.0), RelativePlacement::Above),
                        "column-inward" => (egui::pos2(outer_x, card.outer.origin.y + 80.0), RelativePlacement::BeforeColumn),
                        "outward" => (
                            egui::pos2(
                                if outer_left { card.outer.origin.x + card.outer.size.width + 14.0 } else { card.outer.origin.x - 14.0 },
                                tab.origin.y + 1.0,
                            ),
                            RelativePlacement::AfterColumn,
                        ),
                        "outward-far" => (
                            egui::pos2(
                                if outer_left { card.outer.origin.x + card.outer.size.width + 40.0 } else { card.outer.origin.x - 40.0 },
                                tab.origin.y + 1.0,
                            ),
                            RelativePlacement::AfterColumn,
                        ),
                        _ => (
                            egui::pos2(tab.origin.x + tab.size.width * if direction == Direction::Ltr { 0.75 } else { 0.25 }, tab.origin.y + 10.0),
                            RelativePlacement::Above,
                        ),
                    };
                    let start = raw.at(HostControlTarget::Pointer(HostPointerTarget::Header { instance: id("moving") }));
                    let before = raw.content.runtime.layout().inspect();
                    let commits = raw.content.native.commits;
                    raw.frame(vec![egui::Event::PointerMoved(start)], false);
                    raw.pointer(start, true, false);
                    assert!(raw.host.interaction().is_captured());
                    assert!(raw.frame(vec![egui::Event::PointerMoved(start + egui::vec2(5.99, 0.0))], false).0.preview.is_none());
                    if case == "outward-far" {
                        let outside = egui::pos2(if outer_left { right(card.outer) + 65.0 } else { card.outer.origin.x - 65.0 }, end.y);
                        assert!(
                            raw.frame(vec![egui::Event::PointerMoved(outside)], false).0.preview.unwrap().action.is_none(),
                            "the centre strip remains bounded"
                        );
                    }
                    let frame = raw.frame(vec![egui::Event::PointerMoved(end)], false).0;
                    let preview = frame.preview.as_ref().unwrap();
                    assert!(preview.rejection.is_none(), "{scale}/{direction:?}/{zone:?}/{case}: {preview:?}");
                    assert_eq!(preview.target, Some(HostDropTargetId::Relative { section: section("section-0"), placement }));
                    assert_eq!(preview.targets.len(), 1);
                    let landing = preview.projected.clone().unwrap();
                    assert_eq!(raw.content.runtime.layout().inspect(), before);
                    let released = raw.pointer(end, false, true);
                    assert_eq!(released.events.iter().filter(|event| matches!(event, HostEvent::PreparedCommitted { .. })).count(), 1);
                    assert_eq!(raw.content.native.commits, commits + 1);
                    assert_eq!(raw.content.runtime.layout().inspect().revision, before.revision + 1);
                    raw.frame(Vec::new(), false);
                    assert_host_landing(&raw.host.frame().unwrap().layout, &landing);
                }
            }
        }
    }
}

#[test]
fn raw_host_wider_perimeters_win_over_cards_tabs_and_clear_below_without_fallback() {
    for scale in [1.0, 2.0] {
        for direction in [Direction::Ltr, Direction::Rtl] {
            for zone in [Zone::Left, Zone::Right] {
                for case in ["side-edge", "side-header", "bottom-clear", "corner", "empty", "bottom-tab", "foreign-tab", "refused-bottom"] {
                    let mut h = host_with_specs(|spec| {
                        spec.min_size.height = 40.0;
                        if spec.kind.0 == "fixture.editor" {
                            spec.allowed_zones.remove(&Zone::Bottom);
                        }
                    });
                    h.dispatch(vec![
                        fixture_open("fixture.log", "target", "target", zone, 0).unwrap(),
                        Mutation::Open {
                            kind: PanelKind::new("fixture.log").unwrap(),
                            instance: id("tab-peer"),
                            destination: Some(Destination::JoinTab { section: section("target"), index: 1 }),
                        },
                        Mutation::Open {
                            kind: PanelKind::new(if case == "refused-bottom" { "fixture.editor" } else { "fixture.log" }).unwrap(),
                            instance: id("moving"),
                            destination: Some(Destination::NewSection {
                                section: section("moving"),
                                index: 1,
                                location: SectionLocation::Floating { origin: Point { x: 560.0, y: 400.0 } },
                                size: Size { width: 280.0, height: 240.0 },
                            }),
                        },
                    ])
                    .unwrap();
                    if case == "foreign-tab" {
                        h.dispatch(vec![Mutation::Open {
                            kind: PanelKind::new("fixture.log").unwrap(),
                            instance: id("moving-peer"),
                            destination: Some(Destination::JoinTab { section: section("moving"), index: 1 }),
                        }])
                        .unwrap();
                    }
                    if matches!(case, "bottom-tab" | "foreign-tab") {
                        h.dispatch(vec![
                            fixture_open("fixture.log", "bottom", "bottom", Zone::Bottom, 2).unwrap(),
                            Mutation::Open {
                                kind: PanelKind::new("fixture.log").unwrap(),
                                instance: id("bottom-peer"),
                                destination: Some(Destination::JoinTab { section: section("bottom"), index: 1 }),
                            },
                            Mutation::SetBottomHeight { section: section("bottom"), height: 64.0 },
                        ])
                        .unwrap();
                    }
                    let mut environment = h.runtime.environment().clone();
                    environment.direction = direction;
                    h.runtime.set_environment(environment, &mut h.local).unwrap();
                    let mut raw = RawPanelHost::new(h, scale);
                    // Exercise both actual shell gaps without multiplying all
                    // selection cases by an unrelated rendering-mode matrix.
                    let mode = if matches!(case, "side-edge" | "bottom-clear" | "empty") {
                        crate::ui_kit::LayoutMode::Docked
                    } else {
                        crate::ui_kit::LayoutMode::Floating
                    };
                    mode.publish(&raw.ctx);
                    let gap = crate::ui_kit::Container::for_layout(mode, crate::ui_kit::Surface::Panel).gap();
                    assert_eq!(gap, if mode == crate::ui_kit::LayoutMode::Docked { 0.0 } else { 7.0 });
                    let mut options = priority_host_options();
                    options.metrics.gap = gap;
                    options.metrics.inline_max_height = 0.0;
                    raw.host = WorkspacePanelHost::new(options).unwrap();
                    raw.frame(Vec::new(), false);
                    raw.frame(Vec::new(), false);
                    let initial = raw.host.frame().unwrap().layout.clone();
                    let card = initial.sections.iter().find(|card| card.section == section("target")).unwrap();
                    let physical_left = direction.edge(zone).unwrap() == PhysicalEdge::Left;
                    let side_x = if physical_left { initial.render_domain.origin.x + 12.0 } else { right(initial.render_domain) - 12.0 };
                    let bottom_y = (bottom(initial.capacity) + gap).min(bottom(initial.render_domain)) - 58.0;
                    let (end, expected_zone) = match case {
                        "side-edge" => (egui::pos2(side_x, card.outer.origin.y + 80.0), zone),
                        "side-header" => (egui::pos2(side_x, card.header.origin.y + 10.0), zone),
                        "bottom-clear" => (egui::pos2(middle(card.outer).x, bottom_y), Zone::Bottom),
                        "corner" | "refused-bottom" => (egui::pos2(side_x, bottom_y), Zone::Bottom),
                        "bottom-tab" | "foreign-tab" => {
                            let bottom = initial.sections.iter().find(|card| card.section == section("bottom")).unwrap();
                            let tab = bottom.tabs.first().unwrap().bounds;
                            (
                                egui::pos2(
                                    tab.origin.x + tab.size.width * if direction == Direction::Ltr { 0.25 } else { 0.75 },
                                    tab.origin.y + 10.0,
                                ),
                                Zone::Bottom,
                            )
                        }
                        _ => (
                            egui::pos2(
                                if physical_left { right(initial.render_domain) - 48.0 } else { initial.render_domain.origin.x + 48.0 },
                                500.0,
                            ),
                            if zone == Zone::Left { Zone::Right } else { Zone::Left },
                        ),
                    };
                    let point = Point { x: end.x, y: end.y };
                    let targets = host_drop_targets(raw.content.runtime.layout(), &initial, &id("moving")).unwrap();
                    let side = targets.iter().find(|target| target.id == HostDropTargetId::Dock { zone, before: None }).unwrap();
                    assert_eq!(side.bounds.size.width, 16.0);
                    let bottom = targets.iter().find(|target| target.id == HostDropTargetId::Dock { zone: Zone::Bottom, before: None }).unwrap();
                    assert_eq!(bottom.bounds.size.height, 64.0);
                    if matches!(case, "bottom-tab" | "foreign-tab") {
                        assert!(
                            targets.iter().any(|target| target.priority == 2 && target.bounds.contains(point)),
                            "the perimeter overlaps an actual measured insertion half"
                        );
                    } else if matches!(case, "bottom-clear" | "corner" | "refused-bottom") {
                        assert!(
                            targets.iter().any(|target| matches!(target.id, HostDropTargetId::Relative { placement: RelativePlacement::Below, .. })
                                && target.bounds.contains(point))
                        );
                    } else if matches!(case, "side-edge" | "side-header") {
                        assert!(targets.iter().any(|target| matches!(target.id, HostDropTargetId::Relative { .. }) && target.bounds.contains(point)));
                    }
                    if case == "refused-bottom" {
                        assert!(side.bounds.contains(point));
                        assert!(
                            prepare_host_layout(
                                raw.content.runtime.layout(),
                                &host_scope(&raw.content),
                                &initial,
                                transaction(vec![Mutation::Move { instance: id("moving"), destination: side.destination.clone() }])
                            )
                            .is_ok(),
                            "a lower-priority side landing would be legal"
                        );
                    }
                    let before = raw.content.runtime.layout().inspect();
                    let commits = raw.content.native.commits;
                    let start = raw.at(HostControlTarget::Pointer(if case == "foreign-tab" {
                        HostPointerTarget::Tab { instance: id("moving") }
                    } else {
                        HostPointerTarget::Header { instance: id("moving") }
                    }));
                    raw.frame(vec![egui::Event::PointerMoved(start)], false);
                    raw.pointer(start, true, false);
                    if case == "empty" {
                        let outside = egui::pos2(
                            if physical_left { right(initial.render_domain) - 57.0 } else { initial.render_domain.origin.x + 57.0 },
                            end.y,
                        );
                        assert!(
                            raw.frame(vec![egui::Event::PointerMoved(outside)], false).0.preview.unwrap().action.is_none(),
                            "the empty-side reach ends at 56 logical pixels"
                        );
                    }
                    let (frame, output) = raw.frame(vec![egui::Event::PointerMoved(end)], false);
                    let preview = frame.preview.unwrap();
                    assert_eq!(raw.content.runtime.layout().inspect(), before);
                    assert_eq!(raw.host.frame().unwrap().layout.painted, initial.painted);
                    assert_eq!(raw.host.frame().unwrap().layout.input, initial.input);
                    assert_eq!(raw.host.frame().unwrap().layout.occlusion, initial.occlusion);
                    if case == "refused-bottom" {
                        assert_eq!(preview.target, None, "structural refusal keeps the existing empty-target semantics");
                        assert!(preview.rejection.is_some() && preview.projected.is_none());
                        assert!(preview.targets.is_empty());
                        raw.pointer(end, false, true);
                        assert_eq!(raw.content.runtime.layout().inspect(), before);
                        assert_eq!(raw.content.native.commits, commits);
                    } else {
                        assert_eq!(
                            preview.target,
                            Some(HostDropTargetId::Dock { zone: expected_zone, before: None }),
                            "{scale}/{direction:?}/{zone:?}/{case}: {preview:?}"
                        );
                        assert!(preview.rejection.is_none(), "{preview:?}");
                        assert_eq!(preview.targets.len(), 1);
                        assert!(output_paints_rect(
                            &output,
                            preview.bounds,
                            Some(solidcraft_theme::Tokens::get().form_primary.gamma_multiply(0.12)),
                            None
                        ));
                        let shown = preview.projected.unwrap();
                        let released = raw.pointer(end, false, true);
                        assert_eq!(released.events.iter().filter(|event| matches!(event, HostEvent::PreparedCommitted { .. })).count(), 1);
                        assert_eq!(raw.content.native.commits, commits + 1);
                        raw.frame(Vec::new(), false);
                        assert_host_landing(&raw.host.frame().unwrap().layout, &shown);
                        raw.pointer(end, false, true);
                        assert_eq!(raw.content.runtime.layout().inspect().revision, before.revision + 1);
                    }
                    assert_eq!(raw.content.commands.calls, 0);
                }
            }
        }
    }
}

#[test]
fn raw_host_perimeter_anchors_survive_source_detach_and_commit_the_exact_preview_once() {
    for scale in [1.0, 2.0] {
        for direction in [Direction::Ltr, Direction::Rtl] {
            for zone in [Zone::Left, Zone::Right] {
                for (case, inset) in [("same-column", 12.0), ("sole", 12.0), ("sole", 48.0), ("retained-tab", 12.0)] {
                    let mut h = host_logs(1, zone);
                    if case == "same-column" {
                        h.dispatch(vec![Mutation::Open {
                            kind: PanelKind::new("fixture.log").unwrap(),
                            instance: id("peer"),
                            destination: Some(Destination::Relative {
                                section: section("section-0"),
                                placement: RelativePlacement::Below,
                                new_section: section("peer"),
                                new_column: None,
                                size: Size { width: 280.0, height: 240.0 },
                            }),
                        }])
                        .unwrap();
                        assert_eq!(
                            h.runtime.layout().layout().section(&section("section-0")).unwrap().column,
                            h.runtime.layout().layout().section(&section("peer")).unwrap().column
                        );
                    } else if case == "retained-tab" {
                        h.dispatch(vec![
                            Mutation::Open {
                                kind: PanelKind::new("fixture.log").unwrap(),
                                instance: id("peer"),
                                destination: Some(Destination::JoinTab { section: section("section-0"), index: 1 }),
                            },
                            Mutation::Activate { instance: id("log-0") },
                        ])
                        .unwrap();
                    }
                    let mut environment = h.runtime.environment().clone();
                    environment.direction = direction;
                    h.runtime.set_environment(environment, &mut h.local).unwrap();
                    let mut raw = RawPanelHost::new(h, scale);
                    raw.host = WorkspacePanelHost::new(priority_host_options()).unwrap();
                    raw.frame(Vec::new(), false);
                    raw.frame(Vec::new(), false);
                    let initial = raw.host.frame().unwrap().layout.clone();
                    assert_eq!(initial.sections.first().unwrap().section, section("section-0"));
                    let physical_left = direction.edge(zone).unwrap() == PhysicalEdge::Left;
                    let end = egui::pos2(
                        if physical_left { initial.render_domain.origin.x + inset } else { right(initial.render_domain) - inset },
                        initial.sections[0].outer.origin.y + 80.0,
                    );
                    let targets = host_drop_targets(raw.content.runtime.layout(), &initial, &id("log-0")).unwrap();
                    let target = targets
                        .iter()
                        .find(|target| {
                            target.id == HostDropTargetId::Dock { zone, before: None } && target.bounds.contains(Point { x: end.x, y: end.y })
                        })
                        .unwrap();
                    if case == "sole" {
                        assert!(
                            matches!(&target.destination, Destination::NewSection { section: receiver, location: SectionLocation::Dock { zone: dock }, .. }
                            if *receiver == section("section-0") && *dock == zone)
                        );
                    } else {
                        let anchor = if case == "same-column" { "peer" } else { "section-0" };
                        assert!(
                            matches!(&target.destination, Destination::Relative { section: receiver, placement: RelativePlacement::BeforeColumn, .. }
                            if *receiver == section(anchor))
                        );
                    }
                    let before = raw.content.runtime.layout().inspect();
                    let commits = raw.content.native.commits;
                    let start = raw.at(HostControlTarget::Pointer(HostPointerTarget::Header { instance: id("log-0") }));
                    raw.frame(vec![egui::Event::PointerMoved(start)], false);
                    raw.pointer(start, true, false);
                    let (frame, output) = raw.frame(vec![egui::Event::PointerMoved(end)], false);
                    let preview = frame.preview.unwrap();
                    assert_eq!(
                        preview.target,
                        Some(HostDropTargetId::Dock { zone, before: None }),
                        "{scale}/{direction:?}/{zone:?}/{case}/{inset}: {preview:?}"
                    );
                    assert!(preview.rejection.is_none(), "{preview:?}");
                    assert_eq!(preview.targets.len(), 1);
                    assert!(output_paints_rect(
                        &output,
                        preview.bounds,
                        Some(solidcraft_theme::Tokens::get().form_primary.gamma_multiply(0.12)),
                        None
                    ));
                    let shown = preview.projected.unwrap();
                    assert_eq!(raw.content.runtime.layout().inspect(), before);
                    assert_eq!(raw.content.native.commits, commits);
                    assert_eq!(raw.host.frame().unwrap().layout.painted, initial.painted);
                    assert_eq!(raw.host.frame().unwrap().layout.input, initial.input);
                    assert_eq!(raw.host.frame().unwrap().layout.occlusion, initial.occlusion);
                    let released = raw.pointer(end, false, true);
                    assert_eq!(released.events.iter().filter(|event| matches!(event, HostEvent::PreparedCommitted { .. })).count(), 1);
                    assert_eq!(raw.content.native.commits, commits + 1);
                    raw.frame(Vec::new(), false);
                    assert_host_landing(&raw.host.frame().unwrap().layout, &shown);
                    raw.pointer(end, false, true);
                    assert_eq!(raw.content.native.commits, commits + 1);
                    let expected_revision = if case == "sole" { before.revision } else { before.revision + 1 };
                    assert_eq!(raw.content.runtime.layout().inspect().revision, expected_revision);
                    assert_eq!(raw.content.commands.calls, 0);
                }
            }
        }
    }
}

#[test]
fn raw_host_drag_and_pair_resize_dispatch_once_even_when_egui_repeats_the_release_pass() {
    for (mode, resize) in
        [(crate::ui_kit::LayoutMode::Floating, false), (crate::ui_kit::LayoutMode::Floating, true), (crate::ui_kit::LayoutMode::Docked, true)]
    {
        let h = host_logs(2, Zone::Left);
        let mut raw = RawPanelHost::new(h, 2.0);
        mode.publish(&raw.ctx);
        let gap = crate::ui_kit::Container::for_layout(mode, crate::ui_kit::Surface::Panel).gap();
        raw.host = WorkspacePanelHost::new(HostOptions { metrics: HostMetrics { gap, ..HostMetrics::default() }, ..HostOptions::default() }).unwrap();
        raw.frame(Vec::new(), false);
        let target = if resize {
            HostPointerTarget::Sash { first: section("section-0"), second: section("section-1") }
        } else {
            HostPointerTarget::Header { instance: id("log-0") }
        };
        let start = raw.at(HostControlTarget::Pointer(target.clone()));
        if resize {
            let sash = raw_control(&raw, &HostControlTarget::Pointer(target));
            let layer = raw.ctx.read_response(sash.id).unwrap().layer_id;
            assert_eq!(Some(layer), raw.root_layer, "dock cards and their sash share the root layer in both shell modes");
            assert_eq!(raw.ctx.layer_id_at(start), Some(layer), "the compact zero-gap seam retains its actual widget layer");
        }
        let end = if resize { start + egui::vec2(0.0, 30.0) } else { egui::pos2(1399.0, 600.0) };
        let before = raw.content.runtime.layout().inspect();
        raw.pointer(start, true, false);
        raw.frame(vec![egui::Event::PointerMoved(end)], false);
        assert_eq!(raw.content.runtime.layout().inspect(), before);
        let shown = raw.host.frame().unwrap().preview.clone().unwrap();
        let frame = raw.pointer(end, false, true);
        assert_eq!(raw.content.runtime.layout().inspect().revision, before.revision + 1);
        assert_eq!(frame.events.iter().filter(|event| matches!(event, HostEvent::PreparedCommitted { .. })).count(), 1);
        let action = shown.action.as_ref().unwrap();
        assert!(if resize {
            matches!(one_mutation(action), Mutation::ResizePair { .. })
        } else {
            matches!(one_mutation(action), Mutation::Move { .. })
        });
        raw.frame(Vec::new(), true);
        assert_eq!(raw.content.runtime.layout().inspect().revision, before.revision + 1);
        assert_eq!(raw.content.commands.calls, 0);
    }
}

#[test]
fn raw_host_sash_double_click_resets_neighbours_through_actual_egui_responses() {
    let mut h = host_logs(2, Zone::Bottom);
    h.dispatch(vec![Mutation::ResizePair {
        first: section("section-0"),
        second: section("section-1"),
        first_size: Size { width: 360.0, height: 240.0 },
        second_size: Size { width: 200.0, height: 240.0 },
    }])
    .unwrap();
    let mut raw = RawPanelHost::new(h, 1.0);
    raw.frame(Vec::new(), false);
    let target = HostControlTarget::Pointer(HostPointerTarget::Sash { first: section("section-0"), second: section("section-1") });
    raw.click(target.clone(), false);
    raw.click(target, false);
    raw.frame(Vec::new(), false);
    raw.frame(Vec::new(), false);
    let sections = &raw.content.runtime.layout().layout().sections;
    assert_eq!(
        (sections[0].size.width, sections[1].size.width),
        (698.0, 698.0),
        "reset splits the actual 1396-point allocation at the default ratio"
    );
    assert_eq!((sections[0].size.height, sections[1].size.height), (240.0, 240.0));
    assert_eq!(raw.host.frame().unwrap().layout.sections.iter().map(|card| card.outer.size.width).collect::<Vec<_>>(), vec![698.0, 698.0]);
    assert_eq!(raw.content.commands.calls, 0);
}

#[test]
fn host_tiny_capacity_preserves_collapsed_headers_in_a_bounded_nonoverlapping_scroll_extent() {
    let mut h = host_logs(1, Zone::Left);
    h.dispatch(vec![
        fixture_open("fixture.log", "right", "right", Zone::Right, 1).unwrap(),
        fixture_open("fixture.log", "bottom", "bottom", Zone::Bottom, 2).unwrap(),
    ])
    .unwrap();
    h.dispatch(
        h.runtime.layout().layout().sections.iter().map(|section| Mutation::Collapse { section: section.id.clone(), collapsed: true }).collect(),
    )
    .unwrap();
    for (width, height) in [(32.0, 160.0), (1.0, 1.0)] {
        let layout = host_layout(&h, width, height, Direction::Ltr);
        assert_eq!(layout.sections.len(), 3);
        assert!(layout.sections.iter().all(|section| section.collapsed && section.body.is_none() && section.outer == section.header));
        assert!(layout.overflow.is_empty() && layout.overflow_launcher.is_none() && layout.rails.is_empty());
        assert!(layout.extent.width > width);
        for bounds in layout.painted.iter().chain(&layout.input).chain(&layout.occlusion) {
            assert!(layout.capacity.contains_rect(*bounds));
        }
        for (index, first) in layout.sections.iter().enumerate() {
            for second in layout.sections.iter().skip(index + 1) {
                assert!(!egui_rect(first.outer).intersect(egui_rect(second.outer)).is_positive());
            }
        }
    }
}
#[test]
fn raw_host_bounded_scroll_reaches_the_last_minimum_header_and_its_shared_actions() {
    let mut h = host_logs(24, Zone::Left);
    h.dispatch((0..24).map(|index| Mutation::Collapse { section: section(&format!("section-{index}")), collapsed: true }).collect()).unwrap();
    let mut raw = RawPanelHost::new(h, 2.0);
    raw.capacity.size = Size { width: 480.0, height: 320.0 };
    raw.frame(Vec::new(), false);
    let before = raw.content.runtime.layout().inspect();
    let track = raw.host.frame().unwrap().controls.iter().find(|control| control.target == HostControlTarget::Scroll { horizontal: false }).unwrap();
    assert!(track.bounds.size.width < raw.capacity.size.width / 10.0, "scroll input must not shield the canvas");
    let at = egui_rect(track.bounds).center_bottom() - egui::vec2(0.0, 1.0);
    raw.pointer(at, true, false);
    raw.pointer(at, false, false);
    raw.frame(Vec::new(), false);
    assert_eq!(raw.content.runtime.layout().inspect(), before, "presentation scroll is not layout state");
    let target = HostControlTarget::Pointer(HostPointerTarget::Menu { instance: id("log-23") });
    let header = raw_control(&raw, &target);
    assert!(egui_rect(header.bounds).is_positive() && egui_rect(raw.capacity).contains_rect(egui_rect(header.bounds)));
    raw.menu(id("log-23"));
    assert!(
        raw.host
            .frame()
            .unwrap()
            .controls
            .iter()
            .any(|control| control.target == HostControlTarget::Shared { instance: id("log-23"), action: HostSharedAction::Collapse })
    );
    assert_eq!(raw.content.runtime.controller_count(), 24);
    assert_eq!(raw.content.commands.calls, 0);
}
#[test]
fn host_tab_overflow_accounts_for_every_tab_and_preserves_active_selection() {
    let mut h = host_logs(1, Zone::Left);
    for index in 1..MAX_TABS {
        h.dispatch(vec![Mutation::Open {
            kind: PanelKind::new("fixture.log").unwrap(),
            instance: id(&format!("log-{index}")),
            destination: Some(Destination::JoinTab { section: section("section-0"), index }),
        }])
        .unwrap();
    }
    h.dispatch(vec![
        Mutation::Activate { instance: id("log-31") },
        Mutation::SetSize { section: section("section-0"), size: Size { width: 160.0, height: 240.0 } },
    ])
    .unwrap();
    for collapsed in [false, true] {
        h.dispatch(vec![Mutation::Collapse { section: section("section-0"), collapsed }]).unwrap();
        for direction in [Direction::Ltr, Direction::Rtl] {
            let layout = host_layout(&h, 800.0, 600.0, direction);
            assert_host_geometry(&h, &layout);
            let group = &layout.sections[0];
            let overflow = group.tab_overflow.expect("compact overflow stays reachable");
            assert_eq!(overflow.size.width, layout.metrics.control_extent);
            assert_eq!(group.tabs.len() + group.hidden_tabs.len(), MAX_TABS);
            let all =
                group.tabs.iter().map(|tab| tab.instance.clone()).chain(group.hidden_tabs.iter().cloned()).collect::<std::collections::BTreeSet<_>>();
            assert_eq!(all.len(), MAX_TABS, "neither truncation nor duplicated active tab");
            assert!(group.tabs.iter().any(|tab| tab.instance == id("log-31")), "active tab fits beside compact overflow");
            assert_eq!(layout.resolved.panels[0].instance, id("log-31"));
            assert_eq!(group.tab_strip.unwrap().origin.y, group.header.origin.y);
            let grip = group.title.unwrap();
            assert_eq!(
                if direction == Direction::Ltr { grip.origin.x } else { right(grip) },
                if direction == Direction::Ltr { group.header.origin.x } else { right(group.header) }
            );
        }
    }
}

type HostSlotTrace = std::rc::Rc<std::cell::RefCell<Vec<(ContentSlot, Flow, Option<LogicalRect>, egui::Rect)>>>;

struct LongPanelChrome {
    inner: Box<dyn PanelController>,
    slots: HostSlotTrace,
}

#[derive(Default)]
struct LeafInputState {
    body_text: String,
    overlay_text: String,
    popup_text: String,
    body: Option<egui::Response>,
    overlay: Option<egui::Response>,
    popup: Option<egui::Response>,
}
type LeafInputProbe = std::rc::Rc<std::cell::RefCell<std::collections::BTreeMap<InstanceId, LeafInputState>>>;

/// Reuse the existing controller's lifecycle/commands and add real text-edit
/// leaves. Input ownership comes from their actual egui responses and the same
/// local Overlay requests a controller uses in production.
struct LeafInputController {
    inner: Box<dyn PanelController>,
    leaves: LeafInputProbe,
}

impl PanelController for LeafInputController {
    fn availability(&self, owner: &ContentOwner, models: &dyn PanelModels) -> Availability {
        self.inner.availability(owner, models)
    }
    fn update(&mut self, owner: &ContentOwner, services: &mut ContentServices<'_>) -> PanelResult<()> {
        self.inner.update(owner, services)
    }
    fn presentation(&self) -> PanelPresentation {
        self.inner.presentation()
    }
    fn prepare_lifecycle(&self, event: &LifecycleEvent) -> PanelResult<()> {
        self.inner.prepare_lifecycle(event)
    }
    fn lifecycle(&mut self, event: &LifecycleEvent) {
        self.inner.lifecycle(event);
    }
    fn action(&mut self, owner: &ContentOwner, action: &ControlId, services: &mut ContentServices<'_>) -> PanelResult<()> {
        self.inner.action(owner, action, services)
    }
    fn render(&mut self, slot: ContentSlot, ui: &mut egui::Ui, context: &PanelContext, services: &mut ContentServices<'_>) -> PanelResult<()> {
        self.inner.render(slot, ui, context, services)?;
        if slot != ContentSlot::Body || !ui.is_visible() || ui.is_sizing_pass() {
            return Ok(());
        }
        let mut leaves = self.leaves.borrow_mut();
        let leaf = leaves.entry(context.instance.clone()).or_default();
        leaf.body = Some(crate::ui_kit::cell_editor(ui, ui.scope_id().with("actual_body_leaf"), &mut leaf.body_text, "Body text", false));
        let scope = LocalScope { workspace: context.workspace.clone(), document: context.document.clone(), viewport: context.viewport.clone() };
        let region = if context.instance == id("document-body") {
            // This second controller's overlay is an actual leaf on its mounted
            // Surface layer; it is outside the current left-hand Peek.
            let response =
                crate::ui_kit::cell_editor(ui, ui.scope_id().with("actual_other_overlay"), &mut leaf.overlay_text, "Other owner overlay text", false);
            let bounds = logical_rect(response.rect);
            leaf.overlay = Some(response);
            Some(bounds)
        } else if context.instance == id("editor") {
            // A current-Peek-owned popup extends beyond the card, with explicit
            // local-service metadata rather than title/geometry-based ownership.
            let bounds = LogicalRect { origin: Point { x: 440.0, y: 650.0 }, size: Size { width: 220.0, height: 90.0 } };
            let container = crate::ui_kit::Container::new(ui.ctx(), crate::ui_kit::Surface::Popup);
            let frame = crate::ui_kit::Container::menu_frame();
            container
                .area(ui.scope_id().with("actual_owned_overlay"))
                .order(egui::Order::Foreground)
                .fixed_pos(egui_rect(bounds).min)
                .movable(false)
                .constrain(false)
                .fade_in(false)
                .show(ui.ctx(), |ui| {
                    ui.set_min_size(egui_rect(bounds).size());
                    container.show_frame(ui, frame, egui_rect(bounds), |ui| {
                        leaf.overlay = Some(crate::ui_kit::cell_editor(
                            ui,
                            ui.scope_id().with("actual_owned_overlay_leaf"),
                            &mut leaf.overlay_text,
                            "Peek owner overlay text",
                            false,
                        ));
                    });
                });
            Some(bounds)
        } else {
            None
        };
        if let Some(bounds) = region {
            services.local.request(
                &scope,
                LocalRequest::Overlay {
                    instance: context.instance.clone(),
                    region: InputRegion {
                        control: ControlId::new("actual-leaf-overlay")?,
                        role: InputRole::Overlay,
                        bounds,
                        keyboard_focusable: true,
                        accessible_name: format!("Owned overlay for {}", context.instance.0),
                    },
                },
            )?;
        }
        if context.instance == id("document-body") {
            let bounds = LogicalRect { origin: Point { x: 800.0, y: 650.0 }, size: Size { width: 220.0, height: 90.0 } };
            let container = crate::ui_kit::Container::new(ui.ctx(), crate::ui_kit::Surface::Popup);
            let frame = crate::ui_kit::Container::menu_frame();
            let mut layer = None;
            container
                .area(ui.scope_id().with("actual_other_floating_overlay"))
                .order(egui::Order::Foreground)
                .fixed_pos(egui_rect(bounds).min)
                .movable(false)
                .constrain(false)
                .fade_in(false)
                .show(ui.ctx(), |ui| {
                    ui.set_min_size(egui_rect(bounds).size());
                    if !ui.is_visible() || ui.is_sizing_pass() {
                        return;
                    }
                    container.show_frame(ui, frame, egui_rect(bounds), |ui| {
                        let response = crate::ui_kit::cell_editor(
                            ui,
                            ui.scope_id().with("actual_other_popup_leaf"),
                            &mut leaf.popup_text,
                            "Other owner floating overlay text",
                            false,
                        );
                        layer = Some(response.layer_id);
                        leaf.popup = Some(response);
                    });
                });
            if let Some(layer) = layer {
                services.local.request_overlay_layer(
                    &scope,
                    &context.instance,
                    InputRegion {
                        control: ControlId::new("actual-floating-leaf-overlay")?,
                        role: InputRole::Overlay,
                        bounds,
                        keyboard_focusable: true,
                        accessible_name: "Other owner's actual floating overlay".into(),
                    },
                    layer,
                )?;
            }
        }
        Ok(())
    }
}

fn raw_leaf_fixture(scale: f32) -> (RawPanelHost, LeafInputProbe) {
    let mut content = PanelMountHarness::without_content(false).unwrap();
    let leaves = LeafInputProbe::default();
    for kind in ["fixture.editor", "fixture.log"] {
        let create = fixture_controller_binding(&content.probe, true, Ownership::Document).create;
        let trace = leaves.clone();
        content
            .runtime
            .register(
                PanelKind::new(kind).unwrap(),
                ControllerBinding {
                    ownership: Ownership::Document,
                    create: Box::new(move |instance, owner| {
                        Ok(Box::new(LeafInputController { inner: create(instance, owner)?, leaves: trace.clone() }))
                    }),
                },
            )
            .unwrap();
    }
    open_editor(&mut content);
    content
        .dispatch(vec![
            fixture_open("fixture.log", "document-body", "document-body-section", Zone::Right, 0).unwrap(),
            fixture_open("fixture.log", "next-header", "next-header-section", Zone::Left, 1).unwrap(),
            Mutation::SetSize { section: section("document-body-section"), size: Size { width: 320.0, height: 360.0 } },
            Mutation::Collapse { section: section("editor-section"), collapsed: true },
            Mutation::Collapse { section: section("next-header-section"), collapsed: true },
        ])
        .unwrap();
    let scope = host_scope(&content);
    for instance in &content.runtime.layout().layout().instances {
        content.local.bind(instance.id.clone(), scope.clone()).unwrap();
    }
    let key = editor_key(&content);
    content.probe.edit(&key, |state| state.draft = "Expanded content retains this independent edit".into()).unwrap();
    let mut raw = RawPanelHost::new(content, scale);
    raw.bind_fixture_scopes = true;
    raw.prime_foreign_window();
    raw.frame(Vec::new(), false);
    raw.click(HostControlTarget::Pointer(HostPointerTarget::Collapse { instance: id("editor") }), false);
    raw.frame(Vec::new(), false);
    raw.frame(Vec::new(), false);
    raw.frame(Vec::new(), false);
    assert!(!raw.content.runtime.layout().layout().containing(&id("editor")).unwrap().collapsed);
    assert!(leaves.borrow()[&id("editor")].body.as_ref().unwrap().enabled());
    assert!(leaves.borrow()[&id("editor")].overlay.as_ref().unwrap().enabled());
    let popup = leaves.borrow()[&id("document-body")].popup.clone().expect("the other controller must paint its actual floating text editor");
    assert!(popup.enabled());
    assert!(
        raw.host.frame().unwrap().layout.sections.iter().all(|card| !egui_rect(card.outer).contains(popup.rect.center())),
        "the proven overlay is outside both mounted cards"
    );
    (raw, leaves)
}

#[derive(Clone, Copy, Debug)]
enum LeafRoute {
    OtherBody,
    CurrentBody,
    OwnOverlay,
    OtherOverlay,
    OtherPopup,
    OtherApply,
    ForeignWindow,
    WarmedPopupCover,
    LatePopupCover,
    NextHeader,
}

#[test]
fn real_body_and_overlay_leaves_keep_input_and_retained_headers_under_foreign_layers() {
    for scale in [1.0, 2.0] {
        for route in [
            LeafRoute::OtherBody,
            LeafRoute::CurrentBody,
            LeafRoute::OwnOverlay,
            LeafRoute::OtherOverlay,
            LeafRoute::OtherPopup,
            LeafRoute::OtherApply,
            LeafRoute::ForeignWindow,
            LeafRoute::WarmedPopupCover,
            LeafRoute::LatePopupCover,
            LeafRoute::NextHeader,
        ] {
            let (mut raw, leaves) = raw_leaf_fixture(scale);
            let owner = if matches!(route, LeafRoute::CurrentBody | LeafRoute::OwnOverlay) { id("editor") } else { id("document-body") };
            let leaf = match route {
                LeafRoute::OtherApply | LeafRoute::NextHeader => None,
                LeafRoute::OwnOverlay | LeafRoute::OtherOverlay => leaves.borrow()[&owner].overlay.clone(),
                LeafRoute::OtherPopup | LeafRoute::WarmedPopupCover | LeafRoute::LatePopupCover => leaves.borrow()[&owner].popup.clone(),
                _ => leaves.borrow()[&owner].body.clone(),
            };
            let at = match route {
                LeafRoute::OtherApply => {
                    raw.at(HostControlTarget::Content { instance: id("document-body"), action: ControlId::new("apply").unwrap() })
                }
                LeafRoute::NextHeader => raw.at(HostControlTarget::Pointer(HostPointerTarget::Collapse { instance: id("next-header") })),
                _ => leaf.as_ref().unwrap().rect.center(),
            };
            raw.frame(vec![egui::Event::PointerMoved(at)], false);
            if matches!(route, LeafRoute::ForeignWindow | LeafRoute::WarmedPopupCover) {
                raw.foreign_cover = Some(egui::Rect::from_center_size(at, egui::vec2(140.0, 70.0)));
                raw.frame(Vec::new(), false);
                raw.frame(Vec::new(), false);
            }
            let editor = editor_key(&raw.content);
            let other = ControllerKey { instance: id("document-body"), owner: raw.content.runtime.instance_owner(&id("document-body")).unwrap() };
            let prior_editor = raw.content.probe.state(&editor).unwrap();
            let prior_other = raw.content.probe.state(&other).unwrap();
            let before = raw.content.runtime.layout().inspect();
            let commits = raw.content.native.commits;
            let expected = if matches!(route, LeafRoute::NextHeader) {
                raw.content
                    .runtime
                    .layout()
                    .project(LayoutAction::Apply {
                        expected_revision: Some(before.revision),
                        mutations: vec![Mutation::Collapse { section: section("next-header-section"), collapsed: false }],
                    })
                    .unwrap()
            } else {
                before.layout.clone()
            };
            if matches!(route, LeafRoute::LatePopupCover) {
                raw.foreign_cover = Some(egui::Rect::from_center_size(at, egui::vec2(140.0, 70.0)));
            }
            raw.pointer(at, true, false);
            assert_eq!(raw.content.runtime.layout().inspect(), before, "press alone never changes panel placement");
            if let Some(leaf) = &leaf {
                let actual = raw.ctx.read_response(leaf.id).unwrap();
                if matches!(route, LeafRoute::ForeignWindow | LeafRoute::WarmedPopupCover) {
                    assert!(!actual.is_pointer_button_down_on());
                    assert!(raw.foreign_responses.iter().any(egui::Response::is_pointer_button_down_on));
                } else if !matches!(route, LeafRoute::LatePopupCover) {
                    assert!(actual.is_pointer_button_down_on(), "actual leaf receives {route:?}");
                    assert_eq!(raw.ctx.memory(|memory| memory.focused()), Some(leaf.id));
                }
            }
            raw.pointer(at, false, true);
            if let Some(leaf) = &leaf
                && !matches!(route, LeafRoute::ForeignWindow | LeafRoute::WarmedPopupCover | LeafRoute::LatePopupCover)
            {
                raw.frame(vec![egui::Event::Text("typed into the real leaf".into())], false);
                assert_eq!(raw.ctx.memory(|memory| memory.focused()), Some(leaf.id));
                let inputs = leaves.borrow();
                let value = &inputs[&owner];
                let text = if matches!(route, LeafRoute::OwnOverlay | LeafRoute::OtherOverlay) {
                    &value.overlay_text
                } else if matches!(route, LeafRoute::OtherPopup) {
                    &value.popup_text
                } else {
                    &value.body_text
                };
                assert_eq!(text, "typed into the real leaf");
            }
            raw.pointer(at, false, true);
            raw.frame(Vec::new(), true);
            let restored = matches!(route, LeafRoute::NextHeader);
            assert_eq!(raw.content.runtime.layout().inspect().revision, before.revision + u64::from(restored));
            assert_eq!(raw.content.runtime.layout().layout(), &expected);
            assert_eq!(raw.content.native.commits, commits + u64::from(restored));
            assert!(!raw.host.interaction().is_captured());
            assert_eq!(raw.content.commands.calls, u64::from(matches!(route, LeafRoute::OtherApply)));
            for (key, prior) in [(&editor, &prior_editor), (&other, &prior_other)] {
                let after = raw.content.probe.state(key).unwrap();
                assert_eq!(after.draft, prior.draft);
                assert_eq!(
                    (after.reparents, after.cancels, after.finishes, after.destroys),
                    (prior.reparents, prior.cancels, prior.finishes, prior.destroys)
                );
                assert!(after.transaction_live);
            }
        }
    }
}
fn scroll_real_host_popup_to_end(raw: &mut RawPanelHost) {
    let at = egui_rect(raw.host.frame().unwrap().menu_bounds.unwrap()).center();
    raw.frame(
        vec![
            egui::Event::PointerMoved(at),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, -3000.0),
                modifiers: egui::Modifiers::NONE,
                phase: egui::TouchPhase::Move,
                source: egui::MouseWheelSource::Unknown,
            },
        ],
        false,
    );
    for _ in 0..12 {
        raw.frame(Vec::new(), false);
    }
}

#[derive(Clone, Copy, Debug)]
enum MenuRoute {
    CurrentPanelApply,
    CurrentTabs,
    OtherMove,
    KeyboardMove,
    OtherApply,
    CloseRefusal,
    LateWindow,
    LateModal,
    OtherHeaderMenu,
}

#[test]
fn actual_popup_actions_preserve_retained_owners_and_respect_late_layers_and_lifecycle_refusals() {
    for scale in [1.0, 2.0] {
        for route in [
            MenuRoute::CurrentPanelApply,
            MenuRoute::CurrentTabs,
            MenuRoute::OtherMove,
            MenuRoute::KeyboardMove,
            MenuRoute::OtherApply,
            MenuRoute::CloseRefusal,
            MenuRoute::LateWindow,
            MenuRoute::LateModal,
            MenuRoute::OtherHeaderMenu,
        ] {
            let (mut raw, _) = raw_leaf_fixture(scale);
            if matches!(route, MenuRoute::LateModal) {
                raw.prime_modal();
            }
            if matches!(route, MenuRoute::CurrentTabs) {
                let mut mutations = (0..8)
                    .map(|index| Mutation::Open {
                        kind: PanelKind::new("fixture.log").unwrap(),
                        instance: id(&format!("retained-tab-{index}")),
                        destination: Some(Destination::JoinTab { section: section("editor-section"), index: index + 1 }),
                    })
                    .collect::<Vec<_>>();
                mutations.push(Mutation::Activate { instance: id("editor") });
                raw.content.dispatch(mutations).unwrap();
                raw.frame(Vec::new(), false);
                raw.frame(Vec::new(), false);
                raw.click(HostControlTarget::Pointer(HostPointerTarget::TabOverflow { section: section("editor-section") }), false);
                raw.frame(Vec::new(), false);
                raw.frame(Vec::new(), false);
            } else {
                raw.menu(if matches!(route, MenuRoute::CurrentPanelApply) {
                    id("editor")
                } else if matches!(route, MenuRoute::OtherHeaderMenu) {
                    id("next-header")
                } else {
                    id("document-body")
                });
            }
            assert!(raw.host.frame().unwrap().menu_bounds.is_some());
            if matches!(route, MenuRoute::CurrentPanelApply | MenuRoute::OtherApply | MenuRoute::CloseRefusal) {
                scroll_real_host_popup_to_end(&mut raw);
            }
            let target = match route {
                MenuRoute::CurrentPanelApply => HostControlTarget::Content { instance: id("editor"), action: ControlId::new("apply").unwrap() },
                MenuRoute::CurrentTabs => HostControlTarget::PanelMenu { instance: id("editor") },
                MenuRoute::OtherApply => HostControlTarget::Content { instance: id("document-body"), action: ControlId::new("apply").unwrap() },
                MenuRoute::CloseRefusal => HostControlTarget::Shared { instance: id("document-body"), action: HostSharedAction::Close },
                MenuRoute::OtherHeaderMenu => HostControlTarget::Shared { instance: id("next-header"), action: HostSharedAction::Collapse },
                _ => HostControlTarget::Shared { instance: id("document-body"), action: HostSharedAction::Move(Zone::Bottom) },
            };
            let frame = raw.host.frame().unwrap();
            let menu = frame.menu_bounds.unwrap();
            let source = frame
                .controls
                .iter()
                .find(|control| {
                    control.target == target
                        && menu.contains_rect(control.bounds)
                        && raw.ctx.read_response(control.id).is_some_and(|response| response.layer_id.order == egui::Order::Foreground)
                })
                .unwrap()
                .clone();
            assert!(source.enabled);
            let editor = editor_key(&raw.content);
            let other = ControllerKey { instance: id("document-body"), owner: raw.content.runtime.instance_owner(&id("document-body")).unwrap() };
            if matches!(route, MenuRoute::CloseRefusal) {
                raw.content.probe.edit(&other, |state| state.reject_cancel = true).unwrap();
            }
            let old_editor = raw.content.probe.state(&editor).unwrap();
            let old_other = raw.content.probe.state(&other).unwrap();
            let before = raw.content.runtime.layout().inspect();
            let commits = raw.content.native.commits;
            let at = egui_rect(source.bounds).center();
            let keyboard = matches!(route, MenuRoute::KeyboardMove);
            if keyboard {
                raw.ctx.memory_mut(|memory| memory.request_focus(source.id));
                raw.frame(Vec::new(), false);
            } else {
                raw.frame(vec![egui::Event::PointerMoved(at)], false);
                raw.pointer(at, true, false);
                assert_eq!(raw.content.runtime.layout().inspect(), before);
                assert!(raw.ctx.read_response(source.id).unwrap().is_pointer_button_down_on());
            }
            if matches!(route, MenuRoute::LateWindow) {
                raw.foreign_cover = Some(egui::Rect::from_center_size(at, egui::vec2(140.0, 60.0)));
            }
            if matches!(route, MenuRoute::LateModal) {
                raw.late_modal = true;
            }
            let released = raw
                .frame(
                    if keyboard { vec![host_key(egui::Key::Enter, true)] } else { vec![egui::Event::PointerMoved(at), button_event(at, false)] },
                    true,
                )
                .0;
            let rejected = matches!(route, MenuRoute::CloseRefusal | MenuRoute::LateWindow | MenuRoute::LateModal);
            let structural = !rejected && matches!(route, MenuRoute::OtherMove | MenuRoute::KeyboardMove | MenuRoute::OtherHeaderMenu);
            assert_eq!(raw.content.runtime.layout().inspect().revision, before.revision + u64::from(structural));
            assert_eq!(raw.content.native.commits, commits + u64::from(structural));
            if structural {
                assert_eq!(released.events.iter().filter(|event| matches!(event, HostEvent::Committed { .. })).count(), 1);
                if matches!(route, MenuRoute::OtherHeaderMenu) {
                    assert!(!raw.content.runtime.layout().layout().containing(&id("next-header")).unwrap().collapsed);
                } else {
                    assert_eq!(raw.content.runtime.layout().layout().containing(&id("document-body")).unwrap().location.zone(), Zone::Bottom);
                }
            } else {
                assert_eq!(raw.content.runtime.layout().inspect(), before);
            }
            if matches!(route, MenuRoute::CloseRefusal) {
                assert!(released.events.iter().any(|event| matches!(event, HostEvent::Rejected { error, .. } if error.rule == Rule::Lifecycle)));
            }
            let apply_editor = matches!(route, MenuRoute::CurrentPanelApply);
            let apply_other = matches!(route, MenuRoute::OtherApply);
            assert_eq!(raw.content.commands.calls, u64::from(apply_editor || apply_other));
            assert_eq!(raw.content.probe.state(&editor).unwrap().applies, old_editor.applies + u64::from(apply_editor));
            assert_eq!(raw.content.probe.state(&other).unwrap().applies, old_other.applies + u64::from(apply_other));
            for (key, prior) in [(&editor, &old_editor), (&other, &old_other)] {
                let state = raw.content.probe.state(key).unwrap();
                assert_eq!(state.draft, prior.draft);
                assert_eq!((state.cancels, state.finishes, state.destroys), (prior.cancels, prior.finishes, prior.destroys));
                assert!(state.transaction_live);
            }
            raw.foreign_cover = None;
            raw.late_modal = false;
            raw.frame(vec![host_key(egui::Key::Enter, false)], false);
            raw.pointer(at, false, true);
            raw.frame(Vec::new(), true);
            assert_eq!(raw.content.runtime.layout().inspect().revision, before.revision + u64::from(structural));
            assert_eq!(raw.content.commands.calls, u64::from(apply_editor || apply_other));
        }
    }
}
impl PanelController for LongPanelChrome {
    fn availability(&self, owner: &ContentOwner, models: &dyn PanelModels) -> Availability {
        self.inner.availability(owner, models)
    }
    fn update(&mut self, owner: &ContentOwner, services: &mut ContentServices<'_>) -> PanelResult<()> {
        self.inner.update(owner, services)
    }
    fn presentation(&self) -> PanelPresentation {
        let mut presentation = self.inner.presentation();
        presentation.title = "日本語の長いパネル名 العربية עברית ".repeat(3);
        for action in &mut presentation.actions {
            action.label = format!("{} {}", action.id.0, "Long localized action 日本語 العربية ".repeat(3));
        }
        // Dynamic availability must reach both the footer and shared action menu.
        let finish = presentation.actions.iter_mut().find(|action| action.id.0 == "finish").unwrap();
        finish.enabled = false;
        finish.disabled_reason = Some("Synthetic unavailable content action".into());
        presentation
    }
    fn prepare_lifecycle(&self, event: &LifecycleEvent) -> PanelResult<()> {
        self.inner.prepare_lifecycle(event)
    }
    fn lifecycle(&mut self, event: &LifecycleEvent) {
        self.inner.lifecycle(event);
    }
    fn render(&mut self, slot: ContentSlot, ui: &mut egui::Ui, context: &PanelContext, services: &mut ContentServices<'_>) -> PanelResult<()> {
        self.slots.borrow_mut().push((slot, context.flow, context.usable_body, ui.clip_rect()));
        self.inner.render(slot, ui, context, services)
    }
    fn action(&mut self, owner: &ContentOwner, action: &ControlId, services: &mut ContentServices<'_>) -> PanelResult<()> {
        self.inner.action(owner, action, services)
    }
}

fn host_with_slot_trace() -> (PanelMountHarness, HostSlotTrace) {
    let mut h = PanelMountHarness::without_content(false).unwrap();
    let binding = fixture_controller_binding(&h.probe, true, Ownership::Document);
    let create = binding.create;
    let slots = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let trace = slots.clone();
    h.runtime
        .register(
            PanelKind::new("fixture.editor").unwrap(),
            ControllerBinding {
                ownership: Ownership::Document,
                create: Box::new(move |instance, owner| Ok(Box::new(LongPanelChrome { inner: create(instance, owner)?, slots: trace.clone() }))),
            },
        )
        .unwrap();
    open_editor(&mut h);
    (h, slots)
}

#[test]
fn raw_host_long_localized_chrome_keeps_slots_bounded_and_exposes_scrolled_and_disabled_actions() {
    for scale in [1.0, 2.0] {
        let (mut h, slots) = host_with_slot_trace();
        h.dispatch(vec![Mutation::SetSize { section: section("editor-section"), size: Size { width: 160.0, height: 120.0 } }]).unwrap();
        let mut raw = RawPanelHost::new(h, scale);
        raw.frame(Vec::new(), false);
        assert!(
            raw.host
                .frame()
                .unwrap()
                .controls
                .iter()
                .any(|control| control.target == HostControlTarget::Pointer(HostPointerTarget::Menu { instance: id("editor") })),
            "long title cannot cover the menu"
        );
        for (_, _, body, clip) in slots.borrow().iter() {
            assert!(clip.is_finite() && clip.is_positive());
            let outer = raw.host.frame().unwrap().layout.sections[0].outer;
            if let Some(body) = body {
                assert!(outer.contains_rect(*body));
            }
            assert!(
                egui::Rect::from_min_size(egui::pos2(outer.origin.x, outer.origin.y), egui::vec2(outer.size.width, outer.size.height))
                    .contains_rect(*clip)
            );
        }
        raw.menu(id("editor"));
        let bounds = raw.host.frame().unwrap().menu_bounds.unwrap();
        let at = middle(bounds);
        raw.frame(
            vec![
                egui::Event::PointerMoved(egui::pos2(at.x, at.y)),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -3000.0),
                    modifiers: egui::Modifiers::NONE,
                    phase: egui::TouchPhase::Move,
                    source: egui::MouseWheelSource::Unknown,
                },
            ],
            false,
        );
        for _ in 0..24 {
            raw.frame(Vec::new(), false);
        }
        let finish = HostControlTarget::Content { instance: id("editor"), action: ControlId::new("finish").unwrap() };
        assert!(
            raw.host.frame().unwrap().controls.iter().any(|control| control.target == finish && !control.enabled),
            "disabled actions stay inspectable after menu scroll"
        );
        raw.click(finish, false);
        assert_eq!(raw.content.probe.state(&editor_key(&raw.content)).unwrap().finishes, 0);
        assert_eq!(raw.content.commands.calls, 0);
    }
}

#[test]
fn raw_host_floating_tall_and_wide_content_receives_logical_body_and_retains_controller_identity() {
    let (mut h, slots) = host_with_slot_trace();
    h.dispatch(vec![move_editor(Zone::Floating)]).unwrap();
    let key = editor_key(&h);
    let mut raw = RawPanelHost::new(h, 2.0);
    raw.frame(Vec::new(), false);
    raw.frame(Vec::new(), false);
    let count = raw.content.runtime.controller_count();
    for (size, flow) in [(Size { width: 240.0, height: 600.0 }, Flow::Tall), (Size { width: 600.0, height: 240.0 }, Flow::Wide)] {
        raw.content.dispatch(vec![Mutation::SetSize { section: section("editor-moved"), size }]).unwrap();
        slots.borrow_mut().clear();
        let (frame, _) = raw.frame(Vec::new(), false);
        let geometry = &frame.layout.resolved.panels[0];
        assert_eq!(geometry.outer.size, size, "logical dimensions are not multiplied by scale");
        assert!(geometry.outer.contains_rect(geometry.body.unwrap()));
        let trace = slots.borrow();
        assert!(trace.iter().any(|(slot, _, _, _)| *slot == ContentSlot::Body));
        assert!(trace.iter().all(|(_, actual_flow, body, _)| *actual_flow == flow && *body == geometry.body));
        assert_eq!(raw.content.runtime.controller_count(), count);
        assert_eq!(raw.content.probe.state(&key).unwrap().cancels, 0);
        assert!(raw.content.probe.state(&key).unwrap().transaction_live);
        assert_eq!(frame.layout.viewport, Some(frame.layout.render_domain));
        assert!(frame.layout.reservations.contains(&geometry.outer), "a visible floating card occupies usable space without cropping rendering");
        assert!(!frame.layout.usable.contains_rect(geometry.outer));
    }
}

#[test]
fn raw_host_combined_native_dpi_and_ui_zoom_apply_scale_once_to_geometry_and_threshold() {
    let h = host_logs(1, Zone::Right);
    let mut raw = RawPanelHost::new(h, 1.0);
    raw.native_pixels_per_point = Some(2.0);
    raw.ctx.set_zoom_factor(1.5);
    let (frame, output) = raw.frame(Vec::new(), false);
    assert_eq!(output.pixels_per_point, 3.0, "exercise native DPI and UI zoom together");
    assert_eq!(frame.layout.sections[0].outer.size, Size { width: 280.0, height: 240.0 });
    let at = raw.at(HostControlTarget::Pointer(HostPointerTarget::Header { instance: id("log-0") }));
    raw.pointer(at, true, false);
    assert!(raw.frame(vec![egui::Event::PointerMoved(at + egui::vec2(5.99, 0.0))], false).0.preview.is_none());
    assert!(raw.frame(vec![egui::Event::PointerMoved(at + egui::vec2(6.0, 0.0))], false).0.preview.is_some());
    assert_eq!(raw.content.runtime.layout().layout().sections[0].size, Size { width: 280.0, height: 240.0 });
}

#[test]
fn host_intersected_minimum_and_maximum_are_honoured_at_the_exact_capacity_boundary() {
    let mut h = host_with_specs(|spec| {
        if spec.kind.0 == "fixture.editor" {
            spec.max_size = Size { width: 300.0, height: 400.0 };
        }
        if spec.kind.0 == "fixture.log" {
            spec.min_size = Size { width: 200.0, height: 180.0 };
        }
    });
    open_editor(&mut h);
    h.dispatch(vec![
        Mutation::Open {
            kind: PanelKind::new("fixture.log").unwrap(),
            instance: id("log"),
            destination: Some(Destination::JoinTab { section: section("editor-section"), index: 1 }),
        },
        Mutation::SetSize { section: section("editor-section"), size: Size { width: 200.0, height: 180.0 } },
    ])
    .unwrap();
    let metrics = HostMetrics { header_height: 16.0, footer_height: 16.0, ..HostMetrics::default() };
    let before = h.runtime.layout().inspect();
    // Minima remain mounted on a clipped scroll canvas, with no invented
    // central viewport reservation and no hidden overflow replacement.
    for width in [200.0, 199.0, 160.0] {
        let capacity = LogicalRect { origin: Point { x: 0.0, y: 0.0 }, size: Size { width, height: 180.0 } };
        let layout = resolve_host_layout(h.runtime.layout(), capacity, Direction::Ltr, metrics, &fixture_metric_policy(h.runtime.layout())).unwrap();
        assert_eq!(layout.resolved.panels.len(), 1);
        assert_eq!(layout.extent.width, 200.0_f32.max(width));
        assert_eq!(layout.resolved.panels[0].outer.size.width, 200.0);
        assert_eq!(layout.viewport, Some(capacity));
        assert!(layout.resolved.panels[0].painted.iter().all(|bounds| intersect_rect(capacity, *bounds) == Some(*bounds)));
        assert_eq!(h.runtime.layout().inspect(), before);
    }
    h.dispatch(vec![Mutation::SetSize { section: section("editor-section"), size: Size { width: 300.0, height: 400.0 } }]).unwrap();
    let layout = host_layout(&h, 2000.0, 1600.0, Direction::Ltr);
    assert_eq!(
        layout.resolved.panels[0].outer.size,
        Size { width: 300.0, height: 240.0 },
        "sole Content keeps measured height within composite maxima"
    );
    assert_eq!(h.runtime.layout().layout().section(&section("editor-section")).unwrap().preferred_extent, Some(400.0));
}

#[test]
fn host_metrics_and_public_geometry_reject_hostile_values_before_pointer_arithmetic() {
    let h = host_logs(1, Zone::Left);
    for field in 0..7 {
        let mut metrics = HostMetrics::default();
        match field {
            0 => metrics.header_height = f32::NAN,
            1 => metrics.footer_height = -1.0,
            2 => metrics.control_extent = 0.0,
            3 => metrics.min_viewport.width = f32::INFINITY,
            4 => metrics.gap = f32::NAN,
            5 => metrics.gap = -1.0,
            _ => metrics.gap = 257.0,
        }
        assert_eq!(
            resolve_host_layout(h.runtime.layout(), capacity(), Direction::Ltr, metrics, &HostGeometryPolicy::default()).unwrap_err().rule,
            Rule::Bounds
        );
    }
    let mut layout = host_layout(&h, 1280.0, 800.0, Direction::Ltr);
    let mut interaction = HostInteraction::default();
    host_press(&h, &layout, &mut interaction, HostPointerTarget::Header { instance: id("log-0") });
    layout.capacity.size.width = f32::NAN;
    let update = interaction.update(h.runtime.layout(), &host_scope(&h), &layout, HostPointerEvent::Move { at: Point { x: 640.0, y: 400.0 } });
    assert!(update.action.is_none());
    assert_eq!(update.rejection.unwrap().rule, Rule::Bounds);
    assert!(!interaction.is_captured());
}

#[test]
fn raw_host_header_drag_joins_content_and_body_drags_cannot_rearrange_panels() {
    for scale in [1.0, 2.0] {
        let h = host_logs(2, Zone::Left);
        let mut raw = RawPanelHost::new(h, scale);
        raw.frame(Vec::new(), false);
        let start = raw.at(HostControlTarget::Pointer(HostPointerTarget::Header { instance: id("log-0") }));
        let target = explicit_drop_point(
            &raw.content,
            &raw.host.frame().unwrap().layout,
            &id("log-0"),
            &HostDropTargetId::Tab { section: section("section-1"), before: None },
        );
        let end = egui::pos2(target.x, target.y);
        raw.pointer(start, true, false);
        let preview = raw.frame(vec![egui::Event::PointerMoved(end)], false).0.preview.unwrap();
        assert_join_drag_action(preview.action.as_ref().unwrap(), id("log-0"), section("section-1"), 1);
        let projected = preview.projected.unwrap();
        raw.pointer(end, false, false);
        raw.frame(Vec::new(), false);
        let group = raw.content.runtime.layout().layout().containing(&id("log-0")).unwrap();
        assert_eq!(group.tabs, vec![id("log-1"), id("log-0")]);
        assert_eq!(group.active, id("log-0"), "the explicit drag landing activates the moved tab in the same transaction");
        assert_eq!(raw.content.runtime.layout().layout().sections.len(), 1);
        assert_host_landing(&raw.host.frame().unwrap().layout, &projected);
        let before = raw.content.runtime.layout().inspect();
        let body = raw.host.frame().unwrap().layout.sections[0].body.unwrap();
        let body = middle(body);
        let start = egui::pos2(body.x, body.y);
        raw.pointer(start, true, false);
        raw.frame(vec![egui::Event::PointerMoved(start + egui::vec2(400.0, 200.0))], false);
        raw.pointer(start + egui::vec2(400.0, 200.0), false, false);
        assert_eq!(raw.content.runtime.layout().inspect(), before);
        assert!(!raw.host.interaction().is_captured());
        assert!(raw.host.frame().unwrap().preview.is_none());
    }
}

fn raw_clear_below_host(scale: f32, direction: Direction, zone: Zone, obstacle: &str) -> RawPanelHost {
    let mut h = host_logs(1, zone);
    h.dispatch(vec![Mutation::Open {
        kind: PanelKind::new("fixture.log").unwrap(),
        instance: id("moving"),
        destination: Some(Destination::NewSection {
            section: section("moving"),
            index: 1,
            location: SectionLocation::Floating { origin: Point { x: 560.0, y: 80.0 } },
            size: Size { width: 280.0, height: 240.0 },
        }),
    }])
    .unwrap();
    if obstacle == "bottom" {
        h.dispatch(vec![fixture_open("fixture.log", "obstacle", "obstacle", Zone::Bottom, 2).unwrap()]).unwrap();
    } else if matches!(obstacle, "floating" | "collapsed") {
        h.dispatch(vec![Mutation::Open {
            kind: PanelKind::new("fixture.log").unwrap(),
            instance: id("obstacle"),
            destination: Some(Destination::NewSection {
                section: section("obstacle"),
                index: 2,
                location: SectionLocation::Floating {
                    origin: Point { x: if direction.edge(zone).unwrap() == PhysicalEdge::Left { 20.0 } else { 1100.0 }, y: 650.0 },
                },
                size: Size { width: 280.0, height: 240.0 },
            }),
        }])
        .unwrap();
        if obstacle == "collapsed" {
            h.dispatch(vec![Mutation::Collapse { section: section("obstacle"), collapsed: true }]).unwrap();
        }
    }
    let mut environment = h.runtime.environment().clone();
    environment.direction = direction;
    h.runtime.set_environment(environment, &mut h.local).unwrap();
    let mut raw = RawPanelHost::new(h, scale);
    raw.frame(Vec::new(), false);
    raw.frame(Vec::new(), false);
    raw
}

#[test]
fn raw_below_short_card_previews_and_commits_halfway_through_clear_space_before_actual_obstacles() {
    for scale in [1.0, 2.0] {
        for direction in [Direction::Ltr, Direction::Rtl] {
            for zone in [Zone::Left, Zone::Right] {
                for obstacle in ["none", "bottom", "floating", "collapsed"] {
                    let mut raw = raw_clear_below_host(scale, direction, zone, obstacle);
                    let initial = raw.host.frame().unwrap().layout.clone();
                    let card = initial.sections.iter().find(|card| card.section == section("section-0")).unwrap();
                    let card_bottom = card.outer.origin.y + card.outer.size.height;
                    let end_y = initial
                        .sections
                        .iter()
                        .find(|card| card.section == section("obstacle"))
                        .map_or(initial.capacity.origin.y + initial.capacity.size.height, |card| card.outer.origin.y);
                    let at = Point { x: middle(card.outer).x, y: (card_bottom + end_y) / 2.0 };
                    assert!(at.y > card_bottom + 100.0, "exercise well below the old inside-only band");
                    assert!(!initial.consumes_pointer(at));
                    assert!(!initial.painted.iter().any(|bounds| bounds.contains(at)));
                    let target = HostDropTargetId::Relative { section: section("section-0"), placement: RelativePlacement::Below };
                    let targets = host_drop_targets(raw.content.runtime.layout(), &initial, &id("moving")).unwrap();
                    let below = targets.iter().find(|entry| entry.id == target).unwrap();
                    assert_eq!(below.bounds.origin.y + below.bounds.size.height, end_y);
                    let start = raw.at(HostControlTarget::Pointer(HostPointerTarget::Header { instance: id("moving") }));
                    raw.pointer(start, true, false);
                    // The obstacle boundary itself must never select the extended region.
                    let boundary = egui::pos2(at.x, end_y);
                    let blocked = raw.frame(vec![egui::Event::PointerMoved(boundary)], false).0;
                    assert_ne!(blocked.preview.and_then(|preview| preview.target), Some(target.clone()));
                    let end = egui::pos2(at.x, at.y);
                    let preview = raw.frame(vec![egui::Event::PointerMoved(end)], false).0.preview.unwrap();
                    assert!(preview.rejection.is_none(), "{obstacle}/{zone:?}/{direction:?}: {preview:?}");
                    assert_eq!(preview.target, Some(target));
                    let shown = preview.projected.unwrap();
                    let before = raw.content.runtime.layout().inspect();
                    let frame = raw
                        .frame(
                            vec![egui::Event::PointerButton {
                                pos: end,
                                button: egui::PointerButton::Primary,
                                pressed: false,
                                modifiers: egui::Modifiers::NONE,
                            }],
                            true,
                        )
                        .0;
                    assert_eq!(frame.events.iter().filter(|event| matches!(event, HostEvent::PreparedCommitted { .. })).count(), 1);
                    assert_eq!(raw.content.runtime.layout().inspect().revision, before.revision + 1);
                    raw.frame(Vec::new(), false);
                    assert_host_landing(&raw.host.frame().unwrap().layout, &shown);
                }
            }
        }
    }
}

#[test]
fn raw_drag_title_tracks_only_the_source_text_without_changing_host_state_or_footprints() {
    fn feedback(output: &egui::FullOutput, title: &str, source: egui::Rect, tab: bool, faded: bool, badge: bool) {
        let t = solidcraft_theme::Tokens::get();
        let (mut originals, mut badges, mut others) = (0, 0, 0);
        for clipped in &output.shapes {
            crate::test_support::each_shape(&clipped.shape, &mut |shape| {
                if let egui::Shape::Text(text) = shape {
                    if text.galley.job.text == title {
                        let original = source.contains(text.pos);
                        let opacity = if original && faded { 0.5 } else { 1.0 };
                        let ink = if !original {
                            t.form_ink
                        } else if tab {
                            t.app_bar_text
                        } else {
                            t.form_primary
                        };
                        assert!(text.galley.job.sections.iter().all(|section| {
                            section.format.color == ink.gamma_multiply(opacity)
                                || (tab && original && section.format.color == ink.gamma_multiply(opacity * 0.7))
                        }));
                        if original {
                            originals += 1;
                        } else {
                            badges += 1;
                        }
                    } else if matches!(text.galley.job.text.as_str(), "Specimen peer" | "Specimen other") {
                        let ink = if text.galley.job.text == "Specimen peer" { t.app_bar_text } else { t.form_ink };
                        assert!(text.galley.job.sections.iter().all(|section| section.format.color == ink));
                        others += 1;
                    }
                }
            });
        }
        assert_eq!((originals, badges), (1, usize::from(badge)));
        assert!(others > 0, "retain full ink on other panel/tab headings");
    }
    for (scale, direction) in [(1.0, Direction::Ltr), (2.0, Direction::Rtl)] {
        for group in [false, true] {
            let mut h = PanelMountHarness::new(false).unwrap();
            open_editor(&mut h);
            h.dispatch(vec![fixture_open("fixture.log", "other", "other", Zone::Right, 1).unwrap()]).unwrap();
            if group {
                h.dispatch(vec![
                    Mutation::Open {
                        kind: PanelKind::new("fixture.log").unwrap(),
                        instance: id("peer"),
                        destination: Some(Destination::JoinTab { section: section("editor-section"), index: 1 }),
                    },
                    Mutation::Activate { instance: id("peer") },
                ])
                .unwrap();
            }
            let mut environment = h.runtime.environment().clone();
            environment.direction = direction;
            h.runtime.set_environment(environment, &mut h.local).unwrap();
            let key = editor_key(&h);
            h.probe.edit(&key, |state| state.draft = "retained through cosmetic drag feedback".into()).unwrap();
            let mut raw = RawPanelHost::new(h, scale);
            raw.frame(Vec::new(), false);
            raw.frame(Vec::new(), false);
            let before = raw.content.runtime.layout().inspect();
            let controllers = raw.content.runtime.controller_count();
            let native_commits = raw.content.native.commits;
            let initial = raw.host.frame().unwrap().layout.clone();
            let target = HostControlTarget::Pointer(if group {
                HostPointerTarget::Tab { instance: id("editor") }
            } else {
                HostPointerTarget::Header { instance: id("editor") }
            });
            let source = egui_rect(raw.host.frame().unwrap().controls.iter().find(|control| control.target == target).unwrap().bounds);
            let controls = raw.host.frame().unwrap().controls.iter().map(|control| (control.id, control.bounds)).collect::<Vec<_>>();
            let title = raw.content.runtime.instance_presentation(&id("editor")).unwrap().title;
            let start = raw.at(target);
            raw.pointer(start, true, false);
            let below = raw.frame(vec![egui::Event::PointerMoved(start + egui::vec2(5.99, 0.0))], false);
            feedback(&below.1, &title, source, group, false, false);
            let threshold = raw.frame(vec![egui::Event::PointerMoved(start + egui::vec2(6.01, 0.0))], false);
            assert_eq!(raw.host.interaction().move_source(), Some(&id("editor")));
            assert!(threshold.1.viewport_output.get(&egui::ViewportId::ROOT).unwrap().repaint_delay.is_zero());
            // Source text was already painted before input; the requested next
            // pass dims it, including when there is no candidate animation.
            let at = egui::pos2(700.0, 500.0); // Clear canvas, no landing.
            let held = raw.frame(vec![egui::Event::PointerMoved(at)], false);
            feedback(&held.1, &title, source, group, true, true);
            let invalid = raw.frame(Vec::new(), false);
            assert!(invalid.0.preview.as_ref().unwrap().action.is_none());
            feedback(&invalid.1, &title, source, group, true, true);
            let left = raw.frame(vec![egui::Event::PointerGone], false);
            feedback(&left.1, &title, source, group, true, false);
            assert!(raw.host.interaction().move_position().is_none());
            let returned = raw.frame(vec![egui::Event::PointerMoved(at)], false);
            feedback(&returned.1, &title, source, group, true, true);
            let cancelled = raw.frame(vec![host_key(egui::Key::Escape, true), button_event(at, false)], false);
            assert!(cancelled.1.viewport_output.get(&egui::ViewportId::ROOT).unwrap().repaint_delay.is_zero());
            let restored = raw.frame(vec![host_key(egui::Key::Escape, false)], false);
            feedback(&restored.1, &title, source, group, false, false);
            assert_eq!(raw.content.runtime.layout().inspect(), before);
            assert_eq!(raw.content.runtime.controller_count(), controllers);
            assert_eq!(raw.content.native.commits, native_commits);
            assert_eq!(raw.content.commands.calls, 0);
            assert_eq!(raw.content.probe.state(&key).unwrap().draft, "retained through cosmetic drag feedback");
            let actual = &raw.host.frame().unwrap().layout;
            assert_eq!(actual.sections, initial.sections);
            assert_eq!(actual.painted, initial.painted);
            assert_eq!(actual.input, initial.input);
            assert_eq!(actual.occlusion, initial.occlusion);
            assert_eq!(actual.reservations, initial.reservations);
            assert_eq!(actual.usable, initial.usable);
            assert_eq!(actual.render_domain, initial.render_domain);
            assert_eq!(raw.host.frame().unwrap().controls.iter().map(|control| (control.id, control.bounds)).collect::<Vec<_>>(), controls);
        }
    }
}

#[test]
fn raw_pointer_leave_and_outside_moves_resume_with_a_fresh_candidate_and_release_once() {
    for scale in [1.0, 2.0] {
        for direction in [Direction::Ltr, Direction::Rtl] {
            for leave in ["gone", "negative", "positive"] {
                let mut raw = raw_clear_below_host(scale, direction, Zone::Left, "bottom");
                let initial = raw.host.frame().unwrap().layout.clone();
                let card = initial.sections.iter().find(|card| card.section == section("section-0")).unwrap();
                let old = middle(card.outer);
                let bottom_y = initial.sections.iter().find(|card| card.section == section("obstacle")).unwrap().outer.origin.y;
                let end = egui::pos2(old.x, (card.outer.origin.y + card.outer.size.height + bottom_y) / 2.0);
                let start = raw.at(HostControlTarget::Pointer(HostPointerTarget::Header { instance: id("moving") }));
                raw.pointer(start, true, false);
                let old_preview = raw.frame(vec![egui::Event::PointerMoved(egui::pos2(old.x, old.y))], false).0.preview.unwrap();
                assert!(old_preview.action.is_some());
                let mut events = Vec::new();
                if leave != "gone" {
                    events.push(egui::Event::PointerMoved(if leave == "negative" { egui::pos2(-20.0, -20.0) } else { egui::pos2(1500.0, 1100.0) }));
                }
                events.push(egui::Event::PointerGone);
                let left = raw.frame(events, false).0;
                assert!(raw.ctx.input(|input| input.pointer.primary_down()));
                assert!(raw.host.interaction().is_captured());
                assert!(left.preview.is_none() && raw.host.interaction().preview().is_none());
                assert!(raw.frame(Vec::new(), false).0.preview.is_none());
                let before = raw.content.runtime.layout().inspect();
                let preview = raw.frame(vec![egui::Event::PointerMoved(end)], false).0.preview.unwrap();
                assert!(preview.rejection.is_none());
                assert_ne!(preview.target, old_preview.target);
                assert_eq!(preview.target, Some(HostDropTargetId::Relative { section: section("section-0"), placement: RelativePlacement::Below }));
                assert_eq!(raw.content.runtime.layout().inspect(), before);
                let shown = preview.projected.unwrap();
                let released = raw
                    .frame(
                        vec![egui::Event::PointerButton {
                            pos: end,
                            button: egui::PointerButton::Primary,
                            pressed: false,
                            modifiers: egui::Modifiers::NONE,
                        }],
                        true,
                    )
                    .0;
                assert_eq!(released.events.iter().filter(|event| matches!(event, HostEvent::PreparedCommitted { .. })).count(), 1);
                assert!(!raw.host.interaction().is_captured());
                raw.frame(Vec::new(), false);
                assert_host_landing(&raw.host.frame().unwrap().layout, &shown);
                assert_eq!(raw.content.runtime.layout().inspect().revision, before.revision + 1);
            }
        }
    }
}

#[test]
fn raw_suspended_drag_cancels_on_outside_release_escape_focus_modal_or_window_change() {
    for case in ["release", "escape", "focus", "modal", "window", "primary-up", "release-only"] {
        let mut raw = raw_clear_below_host(1.0, Direction::Ltr, Zone::Left, "none");
        let card = raw.host.frame().unwrap().layout.sections.iter().find(|card| card.section == section("section-0")).unwrap();
        let at = middle(card.outer);
        let end = egui::pos2(at.x, at.y);
        let start = raw.at(HostControlTarget::Pointer(HostPointerTarget::Header { instance: id("moving") }));
        raw.pointer(start, true, false);
        assert!(raw.frame(vec![egui::Event::PointerMoved(end)], false).0.preview.unwrap().action.is_some());
        raw.frame(vec![egui::Event::PointerGone], false);
        let before = raw.content.runtime.layout().inspect();
        match case {
            "release" => {
                raw.pointer(egui::pos2(-20.0, -20.0), false, false);
            }
            "escape" => {
                raw.frame(vec![host_key(egui::Key::Escape, true)], false);
            }
            "focus" => {
                raw.frame(vec![egui::Event::WindowFocused(false)], false);
            }
            "modal" => {
                raw.late_modal = true;
                raw.frame(Vec::new(), false);
                raw.late_modal = false;
            }
            "window" => {
                let mut environment = raw.content.runtime.environment().clone();
                environment.window = HostId::new("reentry.foreign-window").unwrap();
                raw.content.runtime.set_environment(environment, &mut raw.content.local).unwrap();
                raw.frame(Vec::new(), false);
            }
            "primary-up" => {
                // Simulate a backend updating real egui pointer state without
                // forwarding a release event to this host's raw event stream.
                raw.ctx.input_mut(|input| input.pointer = Default::default());
                raw.frame(vec![egui::Event::PointerMoved(end)], false);
            }
            _ => {
                raw.frame(
                    vec![egui::Event::PointerButton {
                        pos: end,
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        modifiers: egui::Modifiers::NONE,
                    }],
                    false,
                );
            }
        }
        assert!(!raw.host.interaction().is_captured(), "{case}");
        assert!(raw.host.interaction().preview().is_none(), "{case}");
        assert!(raw.host.interaction().move_source().is_none() && raw.host.interaction().move_position().is_none(), "{case}");
        raw.frame(vec![egui::Event::PointerMoved(end)], false);
        raw.pointer(end, false, false);
        assert_eq!(raw.content.runtime.layout().inspect(), before, "{case}");
        assert!(raw.host.frame().unwrap().preview.is_none(), "{case}");
    }
}

#[test]
fn raw_host_inactive_tab_widget_drag_obeys_threshold_and_moves_only_its_retained_editor() {
    for scale in [1.0, 2.0] {
        let mut h = PanelMountHarness::new(false).unwrap();
        open_editor(&mut h);
        h.dispatch(vec![
            Mutation::Open {
                kind: PanelKind::new("fixture.log").unwrap(),
                instance: id("source-active"),
                destination: Some(Destination::JoinTab { section: section("editor-section"), index: 1 }),
            },
            Mutation::Activate { instance: id("source-active") },
            fixture_open("fixture.log", "destination-active", "destination", Zone::Right, 1).unwrap(),
            Mutation::Open {
                kind: PanelKind::new("fixture.log").unwrap(),
                instance: id("destination-inactive"),
                destination: Some(Destination::JoinTab { section: section("destination"), index: 1 }),
            },
        ])
        .unwrap();
        let key = editor_key(&h);
        h.probe.edit(&key, |state| state.draft = "inactive editor draft survives its tab drag".into()).unwrap();
        let mut raw = RawPanelHost::new(h, scale);
        let (initial, output) = raw.frame(Vec::new(), false);
        let target = HostControlTarget::Pointer(HostPointerTarget::Tab { instance: id("editor") });
        let widget = initial.controls.iter().find(|control| control.target == target).expect("inactive editor has a published Tab widget");
        assert_eq!(widget.role, InputRole::Tab);
        assert!(widget.enabled);
        assert!(
            output
                .platform_output
                .accesskit_update
                .as_ref()
                .unwrap()
                .nodes
                .iter()
                .any(|(_, node)| { node.role() == egui::accesskit::Role::Tab && node.label() == Some(widget.accessible_name.as_str()) }),
            "published Tab metadata must correspond to an actual egui widget"
        );
        let start = raw.at(target);
        let before = raw.content.runtime.layout().inspect();
        let controllers = raw.content.runtime.controller_count();
        let native_commits = raw.content.native.commits;
        let editor = raw.content.probe.state(&key).unwrap();
        assert!(!raw.content.runtime.state(&key).unwrap().presented, "drag starts from an inactive tab");

        // Only RawInput reaches the renderer here: no typed Press/Move/Release
        // or direct LayoutAction is used to manufacture the gesture outcome.
        raw.frame(vec![egui::Event::PointerMoved(start)], false);
        let pressed = raw.pointer(start, true, false);
        assert!(raw.host.interaction().is_captured());
        assert!(pressed.preview.is_none());
        let jitter = raw.frame(vec![egui::Event::PointerMoved(start + egui::vec2(5.99, 0.0))], false).0;
        assert!(jitter.preview.is_none());
        assert_eq!(raw.content.runtime.layout().inspect(), before);
        let threshold = raw.frame(vec![egui::Event::PointerMoved(start + egui::vec2(6.01, 0.0))], false).0;
        assert!(threshold.preview.is_some(), "the real tab starts at six logical points at scale {scale}");
        assert_eq!(raw.content.runtime.layout().inspect(), before);

        let destination = explicit_drop_point(
            &raw.content,
            &initial.layout,
            &id("editor"),
            &HostDropTargetId::Tab { section: section("destination"), before: None },
        );
        let end = egui::pos2(destination.x, destination.y);
        let (moving, output) = raw.frame(vec![egui::Event::PointerMoved(end)], false);
        let preview = moving.preview.as_ref().unwrap();
        assert!(preview.rejection.is_none());
        assert_eq!(preview.destination, Some(Destination::JoinTab { section: section("destination"), index: 2 }));
        let projected = preview.projected.clone().unwrap();
        assert!(output_paints_rect(&output, preview.bounds, Some(solidcraft_theme::Tokens::get().form_primary.gamma_multiply(0.12)), None));
        assert!(
            output_paints_rect(&output, preview.tab_marker.unwrap(), Some(solidcraft_theme::Tokens::get().form_primary), None),
            "paint the actual inserted-tab edge, not a nominal body highlight"
        );
        assert_eq!(raw.content.runtime.layout().inspect(), before, "preview cannot activate the tab or change layout");
        assert_eq!(raw.content.native.commits, native_commits);
        assert_eq!(raw.content.commands.calls, 0);

        let released = raw.pointer(end, false, true);
        let committed = released
            .events
            .iter()
            .filter_map(|event| if let HostEvent::PreparedCommitted { reply } = event { Some(reply) } else { None })
            .collect::<Vec<_>>();
        assert_eq!(committed.len(), 1, "one real tab drop commits once across repeated egui passes");
        assert_eq!(raw.content.native.commits, native_commits + 1);
        assert!(raw.finished_frames.len() >= 2, "the release really repeats the host callback");
        assert_eq!(
            raw.finished_frames[0].layout.geometry_policy.identity_reservations.replacement_ownership,
            initial.layout.geometry_policy.identity_reservations.replacement_ownership
        );
        assert_ne!(
            released.layout.geometry_policy.identity_reservations.replacement_ownership,
            initial.layout.geometry_policy.identity_reservations.replacement_ownership,
            "the repeated callback recaptured the changed tab ownership and discarded its old geometry"
        );
        let regions = released
            .events
            .iter()
            .filter_map(|event| match event {
                HostEvent::Regions { revision, painted, reservations, .. } => Some((revision, painted, reservations)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(regions.len(), 1, "outcome restoration cannot resurrect an earlier pass's Regions");
        assert_eq!(*regions[0].0, before.revision + 1);
        assert_eq!(*regions[0].0, released.layout.revision);
        assert_eq!(regions[0].1, &released.layout.painted);
        assert_eq!(regions[0].2, &released.layout.reservations);
        assert_join_drag_action(preview.action.as_ref().unwrap(), id("editor"), section("destination"), 2);
        assert_eq!(committed[0].revision, before.revision + 1);
        let next = raw.frame(Vec::new(), true).0;
        assert!(!next.events.iter().any(|event| matches!(event, HostEvent::Committed { .. } | HostEvent::PreparedCommitted { .. })));
        assert_eq!(raw.content.native.commits, native_commits + 1, "the next frame cannot execute the release again");
        let layout = raw.content.runtime.layout().layout();
        let source = layout.section(&section("editor-section")).unwrap();
        assert_eq!(source.tabs, vec![id("source-active")]);
        assert_eq!(source.active, id("source-active"));
        let destination = layout.section(&section("destination")).unwrap();
        assert_eq!(destination.tabs, vec![id("destination-active"), id("destination-inactive"), id("editor")]);
        assert_eq!(destination.active, id("editor"));
        assert_eq!(layout.sections.len(), 2);
        assert_eq!(raw.content.runtime.layout().inspect().revision, before.revision + 1);
        assert_host_landing(&raw.host.frame().unwrap().layout, &projected);
        assert!(!raw.host.interaction().is_captured());
        assert_eq!(raw.content.runtime.controller_count(), controllers);
        let retained = raw.content.probe.state(&key).unwrap();
        assert_eq!(retained.draft, editor.draft);
        assert_eq!(retained.transaction_live, editor.transaction_live);
        assert_eq!(
            (retained.applies, retained.cancels, retained.finishes, retained.reopens, retained.destroys),
            (editor.applies, editor.cancels, editor.finishes, editor.reopens, editor.destroys)
        );
        assert_eq!(retained.reparents, editor.reparents + 1);
        assert!(retained.renders > editor.renders, "the retained inactive editor becomes the visible landing without recreation");
        assert!(retained.updates > editor.updates);
        assert_eq!(raw.content.commands.calls, 0);
    }
}

#[test]
fn raw_host_action_menu_and_close_button_drags_never_capture_panels_or_commit_content() {
    for scale in [1.0, 2.0] {
        for target in [
            HostControlTarget::Content { instance: id("editor"), action: ControlId::new("apply").unwrap() },
            HostControlTarget::Pointer(HostPointerTarget::Menu { instance: id("editor") }),
            HostControlTarget::Shared { instance: id("editor"), action: HostSharedAction::Close },
        ] {
            let mut h = PanelMountHarness::new(false).unwrap();
            open_editor(&mut h);
            let key = editor_key(&h);
            h.probe.edit(&key, |state| state.draft = "buttons cannot drag or commit this draft".into()).unwrap();
            let mut raw = RawPanelHost::new(h, scale);
            raw.frame(Vec::new(), false);
            if matches!(&target, HostControlTarget::Shared { action: HostSharedAction::Close, .. }) {
                raw.menu(id("editor"));
            }
            let control = raw
                .host
                .frame()
                .unwrap()
                .controls
                .iter()
                .find(|control| control.target == target)
                .expect("button must be genuinely painted and published");
            assert_eq!(control.role, InputRole::Action);
            assert!(control.enabled);
            let start = raw.at(target.clone());
            let end = egui::pos2(raw.capacity.origin.x + raw.capacity.size.width * 0.6, raw.capacity.origin.y + raw.capacity.size.height * 0.6);
            let before = raw.content.runtime.layout().inspect();
            let controllers = raw.content.runtime.controller_count();
            let native_commits = raw.content.native.commits;
            let editor = raw.content.probe.state(&key).unwrap();
            raw.frame(vec![egui::Event::PointerMoved(start)], false);
            let assert_protected = |raw: &RawPanelHost, frame: HostFrame| {
                assert!(!raw.host.interaction().is_captured(), "button press/move/release cannot capture a panel: {target:?}");
                assert!(frame.preview.is_none(), "dragging a button cannot preview a panel move: {target:?}, scale {scale}");
                assert!(
                    frame.events.iter().all(|event| !matches!(
                        event,
                        HostEvent::Committed { .. } | HostEvent::PreparedCommitted { .. } | HostEvent::ContentAction { .. }
                    )),
                    "button drag dispatched an action: {target:?}"
                );
                assert_eq!(frame.reply, before);
            };
            let frame = raw.pointer(start, true, false);
            assert_protected(&raw, frame);
            let frame = raw.frame(vec![egui::Event::PointerMoved(start + egui::vec2(8.0, 0.0))], false).0;
            assert_protected(&raw, frame);
            let frame = raw.frame(vec![egui::Event::PointerMoved(end)], false).0;
            assert_protected(&raw, frame);
            let frame = raw.pointer(end, false, true);
            assert_protected(&raw, frame);
            assert!(!raw.host.interaction().is_captured());
            raw.frame(Vec::new(), false);
            assert_eq!(raw.content.runtime.layout().inspect(), before);
            assert_eq!(raw.content.runtime.controller_count(), controllers);
            assert_eq!(raw.content.native.commits, native_commits);
            assert_eq!(raw.content.commands.calls, 0);
            let retained = raw.content.probe.state(&key).unwrap();
            assert_eq!(retained.draft, editor.draft);
            assert_eq!(retained.transaction_live, editor.transaction_live);
            assert_eq!(
                (retained.applies, retained.cancels, retained.finishes, retained.reopens, retained.reparents, retained.destroys),
                (editor.applies, editor.cancels, editor.finishes, editor.reopens, editor.reparents, editor.destroys)
            );
            assert!(retained.updates > editor.updates);
        }
    }
}

#[test]
fn raw_host_rtl_reduced_motion_and_maximum_length_identities_keep_distinct_immediate_controls() {
    let mut h = PanelMountHarness::new(false).unwrap();
    let prefix = "a".repeat(MAX_ID_BYTES - 1);
    let first = id(&format!("{prefix}b"));
    let second = id(&format!("{prefix}c"));
    h.dispatch(vec![
        fixture_open("fixture.log", &first.0, "first", Zone::Left, 0).unwrap(),
        fixture_open("fixture.log", &second.0, "second", Zone::Left, 1).unwrap(),
    ])
    .unwrap();
    let mut environment = h.runtime.environment().clone();
    environment.direction = Direction::Rtl;
    environment.motion = Motion::Reduced;
    h.runtime.set_environment(environment, &mut h.local).unwrap();
    let mut raw = RawPanelHost::new(h, 2.0);
    let (frame, _) = raw.frame(Vec::new(), false);
    let header = |instance: InstanceId| HostControlTarget::Pointer(HostPointerTarget::Header { instance });
    let a = frame.controls.iter().find(|control| control.target == header(first.clone())).unwrap();
    let b = frame.controls.iter().find(|control| control.target == header(second.clone())).unwrap();
    assert_ne!(a.id, b.id, "full typed identity must survive the wire ID length limit");
    assert!(a.bounds.origin.x > raw.capacity.size.width / 2.0, "logical Left paints on the physical right");
    raw.menu(first.clone());
    raw.click(HostControlTarget::Shared { instance: first.clone(), action: HostSharedAction::Collapse }, false);
    assert!(raw.content.runtime.layout().layout().containing(&first).unwrap().collapsed, "reduced motion cannot defer the state commit");
    raw.frame(Vec::new(), false);
    raw.click(HostControlTarget::Pointer(HostPointerTarget::Collapse { instance: first.clone() }), false);
    assert!(!raw.content.runtime.layout().layout().containing(&first).unwrap().collapsed);
    assert_eq!(raw.content.runtime.controller_count(), 2);
    assert_eq!(raw.content.commands.calls, 0);
}

fn id(value: &str) -> InstanceId {
    InstanceId::new(value).unwrap()
}
fn section(value: &str) -> SectionId {
    SectionId::new(value).unwrap()
}
fn capacity() -> LogicalRect {
    LogicalRect { origin: Point { x: 0.0, y: 0.0 }, size: Size { width: 1400.0, height: 1000.0 } }
}

#[test]
fn in_app_reconciliation_does_not_deactivate_a_peer_native_viewport() {
    let mut h = PanelMountHarness::new(true).unwrap();
    open_editor(&mut h);
    h.dispatch(vec![fixture_open("fixture.log", "native-log", "native-section", Zone::Right, 1).unwrap()]).unwrap();
    h.dispatch(vec![Mutation::Move {
        instance: id("native-log"),
        destination: Destination::NewSection {
            section: section("native-section-moved"),
            index: 1,
            location: SectionLocation::Window {
                host: HostId::new("peer-native").unwrap(),
                origin: Point { x: 0.0, y: 0.0 },
                return_to: DockReturn {
                    zone: Zone::Right,
                    index: 1,
                    size: Size { width: 280.0, height: 240.0 },
                    collapsed: false,
                    join: None,
                    section: None,
                    column: None,
                    column_index: 0,
                    column_width: None,
                    weight: 1.0,
                    preferred_extent: None,
                    extent_priority: 0,
                },
            },
            size: Size { width: 280.0, height: 240.0 },
        },
    }])
    .unwrap();
    h.mount(capacity()).unwrap();
    let native_key = ControllerKey { instance: id("native-log"), owner: ContentOwner::Workspace(h.runtime.environment().workspace.clone()) };
    let native_state = h.runtime.state(&native_key).unwrap().clone();
    assert!(native_state.presented && native_state.activated);
    let scope = h.local.scopes.get(&id("native-log")).unwrap().clone();
    h.local.request(&scope, LocalRequest::KeyboardOwner { instance: id("native-log"), control: ControlId::new("draft").unwrap() }).unwrap();
    let mut root_only = resolve_geometry(h.runtime.layout(), capacity(), Direction::Ltr, 48.0).unwrap();
    root_only.panels.retain(|panel| panel.zone != Zone::Window);
    for resolved in [root_only, ResolvedLayout::default()] {
        h.runtime
            .mount_in_app(&resolved, &mut h.host, &mut ContentServices { models: &h.models, commands: &mut h.commands, local: &mut h.local })
            .unwrap();
        assert_eq!(h.runtime.state(&native_key), Some(&native_state), "complete root mount must preserve peer presentation");
        assert_eq!(h.local.keyboard.get(&scope).unwrap().0, id("native-log"));
    }
    assert!(!h.runtime.state(&editor_key(&h)).unwrap().presented, "an empty root frame releases root presentation");
    assert!(h.probe.state(&editor_key(&h)).unwrap().transaction_live);
}

#[test]
fn prepared_host_moves_are_observational_and_retained_sink_revalidates_the_revision() {
    let mut h = PanelMountHarness::new(false).unwrap();
    open_editor(&mut h);
    h.mount(capacity()).unwrap();
    let key = editor_key(&h);
    h.probe
        .edit(&key, |state| {
            state.draft = "unsaved user draft".into();
            state.reject_cancel = true;
        })
        .unwrap();
    let before = h.runtime.layout().inspect();
    let controllers = h.runtime.controller_count();
    let native_commits = h.native.commits;
    let preview = LayoutAction::Apply { expected_revision: Some(before.revision), mutations: vec![move_editor(Zone::Bottom)] };
    for _ in 0..4 {
        let prepared = h.runtime.layout().prepare(preview.clone()).unwrap();
        assert_eq!(prepared.layout().containing(&id("editor")).unwrap().location.zone(), Zone::Bottom);
        assert_eq!(h.runtime.layout().inspect(), before);
        assert_eq!(h.runtime.controller_count(), controllers);
        assert_eq!(h.native.commits, native_commits);
        assert_eq!(h.commands.calls, 0);
        assert_eq!(h.probe.state(&key).unwrap().reparents, 0);
    }
    h.dispatch(vec![Mutation::Collapse { section: section("editor-section"), collapsed: true }]).unwrap();
    let live = h.runtime.layout().inspect();
    let result = {
        let mut services = ContentServices { models: &h.models, commands: &mut h.commands, local: &mut h.local };
        let mut sink = RuntimeActionSink { runtime: &mut h.runtime, native: &mut h.native, services: &mut services };
        sink.dispatch(preview)
    };
    assert_eq!(result.unwrap_err().rule, Rule::Revision);
    assert_eq!(h.runtime.layout().inspect(), live);
    assert_eq!(h.probe.state(&key).unwrap().draft, "unsaved user draft");
    assert_eq!(h.probe.state(&key).unwrap().reparents, 0);
    assert_eq!(h.probe.state(&key).unwrap().cancels, 0);
    assert_eq!(h.commands.calls, 0);
}
fn editor_key(h: &PanelMountHarness) -> ControllerKey {
    ControllerKey { instance: id("editor"), owner: ContentOwner::Document(h.runtime.environment().document.clone().unwrap()) }
}
fn open_editor(h: &mut PanelMountHarness) {
    h.dispatch(vec![fixture_open("fixture.editor", "editor", "editor-section", Zone::Left, 0).unwrap()]).unwrap();
}
fn move_editor(zone: Zone) -> Mutation {
    Mutation::Move {
        instance: id("editor"),
        destination: Destination::NewSection {
            section: section("editor-moved"),
            index: 0,
            location: if zone == Zone::Floating {
                SectionLocation::Floating { origin: Point { x: 50.0, y: 50.0 } }
            } else {
                SectionLocation::Dock { zone }
            },
            size: Size { width: 300.0, height: 280.0 },
        },
    }
}

#[test]
fn mount_harness_uses_real_controller_slots_and_usable_presentation_context() {
    let mut h = PanelMountHarness::new(false).unwrap();
    open_editor(&mut h);
    let resolved = h.mount(capacity()).unwrap();
    assert_eq!(resolved.panels.len(), 1);
    assert!(h.host.slot_calls >= 3);
    assert_eq!(h.host.slot_calls % 3, 0);
    let record = h.host.mounts.back().unwrap();
    assert_eq!(record.context.usable_body.unwrap().size.height, 192.0);
    assert_eq!(record.context.flow, Flow::Tall);
    assert_eq!(record.presentation.actions.len(), 3);
    assert!(h.probe.state(&editor_key(&h)).unwrap().renders >= 1);
    assert!(h.runtime.state(&editor_key(&h)).unwrap().presented);
}

#[test]
fn hiding_tabbing_collapsing_and_reparenting_retain_editor_transaction() {
    let mut h = PanelMountHarness::new(false).unwrap();
    open_editor(&mut h);
    h.mount(capacity()).unwrap();
    let key = editor_key(&h);
    h.probe.edit(&key, |s| s.draft = "retained draft".into()).unwrap();
    h.dispatch(vec![
        Mutation::Open {
            kind: PanelKind::new("fixture.log").unwrap(),
            instance: id("log"),
            destination: Some(Destination::JoinTab { section: section("editor-section"), index: 1 }),
        },
        Mutation::Activate { instance: id("log") },
    ])
    .unwrap();
    h.mount(capacity()).unwrap();
    assert!(!h.runtime.state(&key).unwrap().presented);
    let updates = h.probe.state(&key).unwrap().updates;
    h.dispatch(vec![Mutation::Collapse { section: section("editor-section"), collapsed: true }]).unwrap();
    h.tick().unwrap();
    h.dispatch(vec![move_editor(Zone::Floating)]).unwrap();
    h.tick().unwrap();
    let state = h.probe.state(&key).unwrap();
    assert!(state.transaction_live);
    assert_eq!(state.draft, "retained draft");
    assert_eq!((state.cancels, state.applies, state.finishes), (0, 0, 0));
    assert!(state.updates > updates);
    assert_eq!(h.commands.calls, 0);
    assert_eq!(state.reparents, 1);
}

#[test]
fn explicit_close_cancel_preflight_failure_preserves_whole_transaction() {
    let mut h = PanelMountHarness::new(false).unwrap();
    open_editor(&mut h);
    h.mount(capacity()).unwrap();
    let key = editor_key(&h);
    h.probe.edit(&key, |s| s.reject_cancel = true).unwrap();
    let before = h.runtime.layout().inspect();
    let count = h.runtime.controller_count();
    let commits = h.native.commits;
    let result = h.dispatch(vec![
        Mutation::SetSize { section: section("editor-section"), size: Size { width: 400.0, height: 300.0 } },
        Mutation::Close { instance: id("editor") },
    ]);
    assert_eq!(result.unwrap_err().rule, Rule::Lifecycle);
    assert_eq!(h.runtime.layout().inspect(), before);
    assert_eq!(h.runtime.controller_count(), count);
    assert_eq!(h.native.commits, commits);
    assert!(h.probe.state(&key).unwrap().transaction_live);
    assert_eq!(h.probe.state(&key).unwrap().cancels, 0);
    h.probe.edit(&key, |s| s.reject_cancel = false).unwrap();
    h.dispatch(vec![Mutation::Close { instance: id("editor") }]).unwrap();
    assert_eq!(h.probe.state(&key).unwrap().cancels, 1);
    assert!(!h.probe.state(&key).unwrap().transaction_live);
    h.dispatch(vec![Mutation::Close { instance: id("editor") }]).unwrap();
    assert_eq!(h.probe.state(&key).unwrap().cancels, 1);
    h.dispatch(vec![Mutation::Open { kind: PanelKind::new("fixture.editor").unwrap(), instance: id("editor"), destination: None }]).unwrap();
    assert!(h.probe.state(&key).unwrap().transaction_live);
}

#[test]
fn saved_open_layout_reopens_an_explicitly_closed_editor_before_mount() {
    let mut h = PanelMountHarness::new(false).unwrap();
    open_editor(&mut h);
    h.mount(capacity()).unwrap();
    let saved = h.runtime.layout().layout().clone();
    let key = editor_key(&h);
    h.probe.edit(&key, |s| s.draft = "closed editor draft".into()).unwrap();
    h.dispatch(vec![Mutation::Close { instance: id("editor") }]).unwrap();
    assert!(!h.probe.state(&key).unwrap().transaction_live);
    assert_eq!(h.probe.state(&key).unwrap().cancels, 1);
    h.apply_layout(saved.clone(), None).unwrap();
    assert!(h.probe.state(&key).unwrap().transaction_live);
    assert_eq!(h.probe.state(&key).unwrap().reopens, 1);
    h.mount(capacity()).unwrap();
    assert!(h.runtime.state(&key).unwrap().presented);
    assert_eq!(h.runtime.controller_count(), 1);
    assert_eq!(h.probe.state(&key).unwrap().draft, "closed editor draft");
    // Committed Reopen clears the lifecycle flag; repeated snapshots cannot reset it.
    h.probe.edit(&key, |s| s.reject_reopen = true).unwrap();
    h.apply_layout(saved, None).unwrap();
    assert_eq!(h.probe.state(&key).unwrap().reopens, 1);
}

#[test]
fn snapshot_closed_entries_hide_live_drafts_but_start_initially_dormant_editors_on_open() {
    let mut h = PanelMountHarness::new(false).unwrap();
    open_editor(&mut h);
    h.mount(capacity()).unwrap();
    let saved = h.runtime.layout().layout().clone();
    let key = editor_key(&h);
    h.probe
        .edit(&key, |s| {
            s.draft = "hidden live draft".into();
            s.reject_reopen = true;
        })
        .unwrap();
    let mut hidden = saved.clone();
    hidden.sections.clear();
    hidden.instances.first_mut().unwrap().requested_open = false;
    h.apply_layout(hidden.clone(), None).unwrap();
    h.tick().unwrap();
    assert!(!h.runtime.state(&key).unwrap().requested_open);
    assert!(h.probe.state(&key).unwrap().transaction_live);
    h.apply_layout(saved.clone(), None).unwrap();
    h.mount(capacity()).unwrap();
    assert_eq!(h.probe.state(&key).unwrap().reopens, 0);
    assert_eq!(h.probe.state(&key).unwrap().cancels, 0);
    assert_eq!(h.probe.state(&key).unwrap().draft, "hidden live draft");

    let mut dormant = PanelMountHarness::new(false).unwrap();
    dormant.apply_layout(hidden, None).unwrap();
    dormant.tick().unwrap();
    let dormant_key = editor_key(&dormant);
    assert!(!dormant.probe.state(&dormant_key).unwrap().transaction_live);
    dormant.apply_layout(saved, None).unwrap();
    dormant.mount(capacity()).unwrap();
    assert!(dormant.probe.state(&dormant_key).unwrap().transaction_live);
    assert_eq!(dormant.probe.state(&dormant_key).unwrap().reopens, 1);
}

#[test]
fn snapshot_reopen_refusal_is_atomic_across_retained_documents_and_workspace_switches() {
    let mut h = PanelMountHarness::new(true).unwrap();
    open_editor(&mut h);
    h.dispatch(vec![
        fixture_open("fixture.application", "application", "application-section", Zone::Right, 1).unwrap(),
        fixture_open("fixture.log", "log", "log-section", Zone::Bottom, 2).unwrap(),
    ])
    .unwrap();
    h.tick().unwrap();
    let first = editor_key(&h);
    let mut next = h.runtime.environment().clone();
    next.document = Some(DocumentId::new("second-document").unwrap());
    h.runtime.set_environment(next, &mut h.local).unwrap();
    h.mount(capacity()).unwrap();
    let second = editor_key(&h);
    h.probe.edit(&first, |s| s.draft = "first canceled draft".into()).unwrap();
    h.probe.edit(&second, |s| s.draft = "second canceled draft".into()).unwrap();
    let mut saved = h.runtime.layout().layout().clone();
    saved.workspace = WorkspaceId::new("reopened-workspace").unwrap();
    h.dispatch(vec![Mutation::Close { instance: id("editor") }]).unwrap();
    h.mount(capacity()).unwrap();
    for key in [&first, &second] {
        assert!(!h.probe.state(key).unwrap().transaction_live);
        assert_eq!(h.probe.state(key).unwrap().cancels, 1);
    }
    let scope = h.local.scopes[&id("application")].clone();
    h.local.request(&scope, LocalRequest::KeyboardOwner { instance: id("application"), control: ControlId::new("draft").unwrap() }).unwrap();
    let before = h.runtime.layout().inspect();
    let environment = h.runtime.environment().clone();
    let count = h.runtime.controller_count();
    let keyboard = h.local.keyboard.clone();
    let scopes = h.local.scopes.clone();
    let releases = h.local.releases;
    let commits = h.native.commits;
    let hosts = h.native.hosts.clone();
    h.probe.edit(&second, |s| s.reject_reopen = true).unwrap();
    assert_eq!(h.apply_layout(saved.clone(), None).unwrap_err().rule, Rule::Lifecycle);
    h.probe.edit(&second, |s| s.reject_reopen = false).unwrap();
    h.native.fail_prepare = true;
    assert_eq!(h.apply_layout(saved.clone(), None).unwrap_err().rule, Rule::Capability);
    assert_eq!(h.runtime.layout().inspect(), before);
    assert_eq!(h.runtime.environment(), &environment);
    assert_eq!(h.runtime.controller_count(), count);
    assert_eq!(h.local.keyboard, keyboard);
    assert_eq!(h.local.scopes, scopes);
    assert_eq!(h.local.releases, releases);
    assert_eq!(h.native.commits, commits);
    assert_eq!(h.native.hosts, hosts);
    for key in [&first, &second] {
        assert_eq!(h.probe.state(key).unwrap().reopens, 0);
        assert!(!h.probe.state(key).unwrap().transaction_live);
    }
    h.native.fail_prepare = false;
    h.apply_layout(saved, None).unwrap();
    h.mount(capacity()).unwrap();
    assert_eq!(h.runtime.controller_count(), count + 1, "only the destination workspace log is new");
    for (key, draft) in [(&first, "first canceled draft"), (&second, "second canceled draft")] {
        let state = h.probe.state(key).unwrap();
        assert!(state.transaction_live);
        assert_eq!(state.reopens, 1);
        assert_eq!(state.cancels, 1);
        assert_eq!(state.draft, draft);
    }
}

#[test]
fn snapshot_reopen_targets_the_destination_workspace_owner() {
    let mut h = PanelMountHarness::new(false).unwrap();
    h.dispatch(vec![fixture_open("fixture.log", "log", "log-section", Zone::Left, 0).unwrap()]).unwrap();
    let first_layout = h.runtime.layout().layout().clone();
    let first = ControllerKey { instance: id("log"), owner: ContentOwner::Workspace(first_layout.workspace.clone()) };
    h.dispatch(vec![Mutation::Close { instance: id("log") }]).unwrap();
    let mut second_layout = first_layout.clone();
    second_layout.workspace = WorkspaceId::new("second-workspace").unwrap();
    h.apply_layout(second_layout.clone(), None).unwrap();
    let second = ControllerKey { instance: id("log"), owner: ContentOwner::Workspace(second_layout.workspace.clone()) };
    h.dispatch(vec![Mutation::Close { instance: id("log") }]).unwrap();
    h.apply_layout(first_layout, None).unwrap();
    assert_eq!(h.probe.state(&first).unwrap().reopens, 1);
    assert_eq!(h.probe.state(&second).unwrap().reopens, 0);
    h.apply_layout(second_layout, None).unwrap();
    assert_eq!(h.probe.state(&first).unwrap().reopens, 1);
    assert_eq!(h.probe.state(&second).unwrap().reopens, 1);
}

#[test]
fn contextual_unavailability_retains_dormant_editor_and_background_updates() {
    let mut h = PanelMountHarness::new(false).unwrap();
    open_editor(&mut h);
    h.mount(capacity()).unwrap();
    let key = editor_key(&h);
    let before = h.runtime.layout().layout().clone();
    h.models.editable = false;
    h.tick().unwrap();
    assert_eq!(h.runtime.layout().layout(), &before);
    assert!(matches!(h.runtime.state(&key).unwrap().available, Availability::Unavailable { .. }));
    assert!(!h.runtime.state(&key).unwrap().presented);
    assert_eq!(h.dispatch(vec![Mutation::Focus { instance: id("editor") }]).unwrap_err().rule, Rule::Availability);
    let updates = h.probe.state(&key).unwrap().updates;
    h.tick().unwrap();
    assert!(h.probe.state(&key).unwrap().updates > updates);
    h.models.editable = true;
    h.mount(capacity()).unwrap();
    assert!(h.runtime.state(&key).unwrap().presented);
    assert!(h.probe.state(&key).unwrap().transaction_live);
    assert_eq!(h.probe.state(&key).unwrap().cancels, 0);
}

#[test]
fn unavailable_new_editor_does_not_commit_layout_or_controller() {
    let mut h = PanelMountHarness::new(false).unwrap();
    h.models.editable = false;
    let before = h.runtime.layout().inspect();
    assert_eq!(
        h.dispatch(vec![fixture_open("fixture.editor", "editor", "editor-section", Zone::Left, 0).unwrap()]).unwrap_err().rule,
        Rule::Availability
    );
    assert_eq!(h.runtime.layout().inspect(), before);
    assert_eq!(h.runtime.controller_count(), 0);
}

#[test]
fn document_switch_retains_independent_editors_and_does_not_duplicate_layout() {
    let mut h = PanelMountHarness::new(false).unwrap();
    open_editor(&mut h);
    h.mount(capacity()).unwrap();
    let first_key = editor_key(&h);
    h.probe.edit(&first_key, |s| s.draft = "document one draft".into()).unwrap();
    let original = h.runtime.environment().clone();
    let layout = h.runtime.layout().layout().clone();
    let mut next = original.clone();
    next.document = Some(DocumentId::new("another-document").unwrap());
    h.runtime.set_environment(next, &mut h.local).unwrap();
    h.tick().unwrap();
    assert_eq!(h.runtime.controller_count(), 2);
    assert_eq!(h.runtime.layout().layout(), &layout);
    let second_key = editor_key(&h);
    h.probe.edit(&second_key, |s| s.draft = "document two draft".into()).unwrap();
    let updates = h.probe.state(&first_key).unwrap().updates;
    h.tick().unwrap();
    assert!(h.probe.state(&first_key).unwrap().updates > updates);
    h.runtime.set_environment(original, &mut h.local).unwrap();
    h.mount(capacity()).unwrap();
    assert_eq!(h.runtime.controller_count(), 2);
    assert_eq!(h.probe.state(&first_key).unwrap().draft, "document one draft");
    assert_eq!(h.probe.state(&second_key).unwrap().draft, "document two draft");
    assert_eq!(h.probe.state(&first_key).unwrap().cancels, 0);
}

fn document_editor_harness() -> PanelMountHarness {
    let mut h = PanelMountHarness::without_content(false).unwrap();
    for (kind, editor, ownership) in [
        ("fixture.application", false, Ownership::Application),
        ("fixture.log", true, Ownership::Document),
        ("fixture.pin", false, Ownership::Workspace),
    ] {
        h.runtime.register(PanelKind::new(kind).unwrap(), fixture_controller_binding(&h.probe, editor, ownership)).unwrap();
    }
    h
}

#[test]
fn all_open_documents_retain_four_editors_plus_application_and_workspace_services() {
    let mut h = document_editor_harness();
    h.dispatch(vec![
        fixture_open("fixture.application", "application", "application-section", Zone::Right, 0).unwrap(),
        fixture_open("fixture.pin", "workspace", "workspace-section", Zone::Left, 1).unwrap(),
        fixture_open("fixture.log", "document-0", "document-section-0", Zone::Floating, 2).unwrap(),
        fixture_open("fixture.log", "document-1", "document-section-1", Zone::Floating, 3).unwrap(),
        fixture_open("fixture.log", "document-2", "document-section-2", Zone::Floating, 4).unwrap(),
        fixture_open("fixture.log", "document-3", "document-section-3", Zone::Floating, 5).unwrap(),
    ])
    .unwrap();
    let saved = h.runtime.layout().layout().clone();
    let original = h.runtime.environment().clone();
    let app_key = ControllerKey { instance: id("application"), owner: ContentOwner::Application };
    let workspace_key = ControllerKey { instance: id("workspace"), owner: ContentOwner::Workspace(saved.workspace.clone()) };
    let mut editor_keys = Vec::new();
    assert_eq!(MAX_RETAINED_DOCUMENT_OWNERS, crate::documents::MAX_OPEN);
    for document in 0..crate::documents::MAX_OPEN {
        let mut environment = original.clone();
        environment.document = Some(DocumentId::new(format!("retained-document-{document}")).unwrap());
        // The initial fixture owner counts toward the same 32-document budget.
        if document == 0 {
            environment = original.clone();
        }
        h.runtime.set_environment(environment, &mut h.local).unwrap();
        h.tick().unwrap();
        for panel in 0..4 {
            let key = ControllerKey {
                instance: id(&format!("document-{panel}")),
                owner: ContentOwner::Document(h.runtime.environment().document.clone().unwrap()),
            };
            h.probe
                .edit(&key, |s| {
                    s.draft = format!("document {document}, panel {panel}");
                    s.reject_cancel = true;
                    s.reject_reopen = true;
                })
                .unwrap();
            editor_keys.push(key);
        }
    }
    assert_eq!(h.runtime.controller_count(), crate::documents::MAX_OPEN * 4 + 2);
    assert!(h.runtime.controller_count() > MAX_INSTANCES);
    assert_eq!(h.runtime.layout().layout(), &saved);
    h.apply_layout(Layout::empty(saved.workspace.clone()), None).unwrap();
    let keys: Vec<_> = editor_keys.iter().cloned().chain([app_key, workspace_key]).collect();
    let updates: Vec<_> = keys.iter().map(|key| h.probe.state(key).unwrap().updates).collect();
    for _ in 0..3 {
        h.tick().unwrap();
    }
    for (key, before) in keys.iter().zip(updates) {
        assert_eq!(h.probe.state(key).unwrap().updates, before + 3);
        assert!(!h.runtime.state(key).unwrap().presented);
    }
    h.apply_layout(saved.clone(), None).unwrap();
    h.runtime.set_environment(original, &mut h.local).unwrap();
    h.mount(capacity()).unwrap();
    for (index, key) in editor_keys.iter().enumerate() {
        let state = h.probe.state(key).unwrap();
        assert_eq!(state.draft, format!("document {}, panel {}", index / 4, index % 4));
        assert!(state.transaction_live);
        assert_eq!((state.cancels, state.reopens, state.destroys), (0, 0, 0));
    }
    assert_eq!(h.runtime.controller_count(), crate::documents::MAX_OPEN * 4 + 2);
    assert_eq!(h.commands.calls, 0);
}

#[test]
fn retained_document_owner_capacity_refusal_preserves_environment_layout_and_local_input() {
    let mut h = document_editor_harness();
    h.dispatch(vec![fixture_open("fixture.log", "editor", "editor-section", Zone::Left, 0).unwrap()]).unwrap();
    let original = h.runtime.environment().clone();
    for document in 1..MAX_RETAINED_DOCUMENT_OWNERS {
        let mut environment = original.clone();
        environment.document = Some(DocumentId::new(format!("document-{document}")).unwrap());
        h.runtime.set_environment(environment, &mut h.local).unwrap();
    }
    h.mount(capacity()).unwrap();
    let key = editor_key(&h);
    h.probe.edit(&key, |s| s.draft = "at document capacity".into()).unwrap();
    let scope = h.local.scopes[&id("editor")].clone();
    h.local.request(&scope, LocalRequest::KeyboardOwner { instance: id("editor"), control: ControlId::new("draft").unwrap() }).unwrap();
    let before = h.runtime.layout().inspect();
    let environment = h.runtime.environment().clone();
    let count = h.runtime.controller_count();
    let keyboard = h.local.keyboard.clone();
    let scopes = h.local.scopes.clone();
    let releases = h.local.releases;
    let commits = h.native.commits;
    let mut excess = environment.clone();
    excess.document = Some(DocumentId::new("excess-document").unwrap());
    assert_eq!(h.runtime.set_environment(excess, &mut h.local).unwrap_err().rule, Rule::CollectionLimit);
    assert_eq!(h.runtime.environment(), &environment);
    assert_eq!(h.runtime.layout().inspect(), before);
    assert_eq!(h.runtime.controller_count(), count);
    assert_eq!(h.local.keyboard, keyboard);
    assert_eq!(h.local.scopes, scopes);
    assert_eq!(h.local.releases, releases);
    assert_eq!(h.native.commits, commits);
    h.tick().unwrap();
    assert!(h.runtime.update_errors().is_empty());
    assert_eq!(h.probe.state(&key).unwrap().draft, "at document capacity");
    assert!(h.probe.state(&key).unwrap().transaction_live);
    // Returning to a retained open owner succeeds at capacity, without eviction.
    h.runtime.set_environment(original, &mut h.local).unwrap();
    h.tick().unwrap();
    assert_eq!(h.runtime.controller_count(), MAX_RETAINED_DOCUMENT_OWNERS);
}

#[test]
fn retained_workspace_owner_capacity_refusal_is_an_atomic_snapshot_failure() {
    let mut h = PanelMountHarness::new(false).unwrap();
    h.dispatch(vec![
        fixture_open("fixture.application", "application", "application-section", Zone::Left, 0).unwrap(),
        fixture_open("fixture.log", "log", "log-section", Zone::Right, 1).unwrap(),
    ])
    .unwrap();
    let saved = h.runtime.layout().layout().clone();
    // The application consumes one non-document owner; workspaces share the rest.
    for workspace in 1..MAX_RETAINED_OTHER_OWNERS - 1 {
        let mut layout = saved.clone();
        layout.workspace = WorkspaceId::new(format!("retained-workspace-{workspace}")).unwrap();
        h.apply_layout(layout, None).unwrap();
    }
    h.mount(capacity()).unwrap();
    assert_eq!(h.runtime.controller_count(), MAX_RETAINED_OTHER_OWNERS);
    let scope = h.local.scopes[&id("application")].clone();
    h.local.request(&scope, LocalRequest::KeyboardOwner { instance: id("application"), control: ControlId::new("draft").unwrap() }).unwrap();
    let before = h.runtime.layout().inspect();
    let environment = h.runtime.environment().clone();
    let keyboard = h.local.keyboard.clone();
    let scopes = h.local.scopes.clone();
    let releases = h.local.releases;
    let commits = h.native.commits;
    let mut excess = saved.clone();
    excess.workspace = WorkspaceId::new("excess-workspace").unwrap();
    assert_eq!(h.apply_layout(excess, None).unwrap_err().rule, Rule::CollectionLimit);
    assert_eq!(h.runtime.layout().inspect(), before);
    assert_eq!(h.runtime.environment(), &environment);
    assert_eq!(h.runtime.controller_count(), MAX_RETAINED_OTHER_OWNERS);
    assert_eq!(h.local.keyboard, keyboard);
    assert_eq!(h.local.scopes, scopes);
    assert_eq!(h.local.releases, releases);
    assert_eq!(h.native.commits, commits);
    h.apply_layout(saved, None).unwrap();
    h.tick().unwrap();
    assert_eq!(h.runtime.controller_count(), MAX_RETAINED_OTHER_OWNERS);
}

#[test]
fn retained_controller_capacity_per_owner_does_not_increase_the_layout_instance_limit() {
    let mut h = document_editor_harness();
    let mutations: Vec<_> = (0..MAX_INSTANCES)
        .map(|index| {
            let group = index / MAX_TABS;
            let instance = id(&format!("editor-{index}"));
            let section_id = section(&format!("editors-{group}"));
            if index % MAX_TABS == 0 {
                fixture_open("fixture.log", &instance.0, &section_id.0, Zone::Left, group).unwrap()
            } else {
                Mutation::Open {
                    kind: PanelKind::new("fixture.log").unwrap(),
                    instance,
                    destination: Some(Destination::JoinTab { section: section_id, index: index % MAX_TABS }),
                }
            }
        })
        .collect();
    for batch in mutations.chunks(MAX_MUTATIONS) {
        h.dispatch(batch.to_vec()).unwrap();
    }
    h.mount(capacity()).unwrap();
    assert_eq!(h.runtime.controller_count(), MAX_INSTANCES);
    let first = ControllerKey { instance: id("editor-0"), owner: ContentOwner::Document(h.runtime.environment().document.clone().unwrap()) };
    h.probe.edit(&first, |s| s.draft = "full owner draft".into()).unwrap();
    let before = h.runtime.layout().inspect();
    let commits = h.native.commits;
    let releases = h.local.releases;
    // One visible layout instance still cannot introduce a 129th retained
    // identity/controller alongside this owner's hidden drafts. The engine's
    // identity cap and runtime's per-owner bound stay aligned.
    let mut source = fixture_service(false).unwrap();
    source.dispatch(transaction(vec![fixture_open("fixture.log", "excess-editor", "excess-section", Zone::Left, 0).unwrap()])).unwrap();
    assert_eq!(h.apply_layout(source.layout().clone(), None).unwrap_err().rule, Rule::CollectionLimit);
    assert_eq!(h.runtime.layout().inspect(), before);
    assert_eq!(h.runtime.controller_count(), MAX_INSTANCES);
    assert_eq!(h.native.commits, commits);
    assert_eq!(h.local.releases, releases);
    assert_eq!(h.probe.state(&first).unwrap().draft, "full owner draft");
    assert!(h.probe.state(&first).unwrap().transaction_live);
    assert_eq!(h.probe.state(&first).unwrap().destroys, 0);
    assert_eq!(
        h.dispatch(vec![fixture_open("fixture.log", "excess-editor", "excess-section", Zone::Left, 4).unwrap()]).unwrap_err().rule,
        Rule::CollectionLimit
    );
    h.apply_layout(before.layout, None).unwrap();
    h.tick().unwrap();
    assert_eq!(h.runtime.controller_count(), MAX_INSTANCES);
}

#[test]
fn multi_instance_controllers_have_independent_identity_and_service_lifetimes() {
    let mut h = PanelMountHarness::new(false).unwrap();
    h.dispatch(vec![
        fixture_open("fixture.log", "log-one", "logs-one", Zone::Left, 0).unwrap(),
        fixture_open("fixture.log", "log-two", "logs-two", Zone::Right, 1).unwrap(),
    ])
    .unwrap();
    h.mount(capacity()).unwrap();
    assert_eq!(h.runtime.controller_count(), 2);
    let first = ControllerKey { instance: id("log-one"), owner: ContentOwner::Workspace(h.runtime.environment().workspace.clone()) };
    let second = ControllerKey { instance: id("log-two"), owner: first.owner.clone() };
    h.dispatch(vec![Mutation::Close { instance: id("log-one") }]).unwrap();
    h.tick().unwrap();
    assert!(!h.runtime.state(&first).unwrap().requested_open);
    assert!(h.runtime.state(&second).unwrap().requested_open);
    assert!(h.probe.state(&first).unwrap().updates > 0);
    h.dispatch(vec![Mutation::Destroy { instance: id("log-one") }]).unwrap();
    assert_eq!(h.runtime.controller_count(), 1);
    assert_eq!(h.probe.state(&first).unwrap().destroys, 1);
}

#[test]
fn native_preflight_failure_and_close_redock_preserve_controller_identity() {
    let mut h = PanelMountHarness::new(true).unwrap();
    open_editor(&mut h);
    h.mount(capacity()).unwrap();
    let key = editor_key(&h);
    let before = h.runtime.layout().inspect();
    let mutation = Mutation::Move {
        instance: id("editor"),
        destination: Destination::NewSection {
            section: section("native-editor"),
            index: 0,
            location: SectionLocation::Window {
                host: HostId::new("native-host").unwrap(),
                origin: Point { x: 0.0, y: 0.0 },
                return_to: DockReturn {
                    zone: Zone::Left,
                    index: 0,
                    size: Size { width: 280.0, height: 240.0 },
                    collapsed: false,
                    join: None,
                    section: None,
                    column: None,
                    column_index: 0,
                    column_width: None,
                    weight: 1.0,
                    preferred_extent: None,
                    extent_priority: 0,
                },
            },
            size: Size { width: 400.0, height: 300.0 },
        },
    };
    h.native.fail_prepare = true;
    assert_eq!(h.dispatch(vec![mutation.clone()]).unwrap_err().rule, Rule::Capability);
    assert_eq!(h.runtime.layout().inspect(), before);
    assert!(h.native.hosts.is_empty());
    assert_eq!(h.probe.state(&key).unwrap().reparents, 0);
    h.native.fail_prepare = false;
    h.dispatch(vec![mutation]).unwrap();
    assert_eq!(h.native.hosts.len(), 1);
    h.dispatch(vec![Mutation::NativeClose { host: HostId::new("native-host").unwrap() }]).unwrap();
    assert!(h.native.hosts.is_empty());
    assert_eq!(h.runtime.controller_count(), 1);
    let state = h.probe.state(&key).unwrap();
    assert!(state.transaction_live);
    assert_eq!(state.cancels, 0);
    assert_eq!(state.reparents, 2);
}

#[test]
fn content_apply_finish_and_cancel_are_separate_from_layout_actions() {
    let mut h = PanelMountHarness::new(false).unwrap();
    open_editor(&mut h);
    h.tick().unwrap();
    let key = editor_key(&h);
    h.commands.fail = true;
    assert!(
        h.runtime
            .content_action(
                &id("editor"),
                &ControlId::new("apply").unwrap(),
                &mut ContentServices { models: &h.models, commands: &mut h.commands, local: &mut h.local }
            )
            .is_err()
    );
    assert_eq!(h.probe.state(&key).unwrap().applies, 0);
    h.commands.fail = false;
    h.runtime
        .content_action(
            &id("editor"),
            &ControlId::new("apply").unwrap(),
            &mut ContentServices { models: &h.models, commands: &mut h.commands, local: &mut h.local },
        )
        .unwrap();
    assert_eq!(h.commands.calls, 1);
    assert!(h.probe.state(&key).unwrap().transaction_live);
    h.runtime
        .content_action(
            &id("editor"),
            &ControlId::new("finish").unwrap(),
            &mut ContentServices { models: &h.models, commands: &mut h.commands, local: &mut h.local },
        )
        .unwrap();
    assert_eq!(h.commands.calls, 2);
    assert!(!h.probe.state(&key).unwrap().transaction_live);
    assert!(h.runtime.layout().layout().instance(&id("editor")).unwrap().requested_open);
}

#[test]
fn local_keyboard_drops_overlays_clipboard_and_picking_reject_wrong_scope() {
    let mut local = FakeLocalServices::default();
    let scope = LocalScope {
        workspace: WorkspaceId::new("workspace").unwrap(),
        document: Some(DocumentId::new("document").unwrap()),
        viewport: ViewportId::new("viewport").unwrap(),
    };
    local.bind(id("one"), scope.clone()).unwrap();
    local.bind(id("two"), scope.clone()).unwrap();
    let keyboard = LocalRequest::KeyboardOwner { instance: id("one"), control: ControlId::new("editor").unwrap() };
    local.request(&scope, keyboard).unwrap();
    assert_eq!(
        local.request(&scope, LocalRequest::KeyboardOwner { instance: id("two"), control: ControlId::new("editor").unwrap() }).unwrap_err().rule,
        Rule::Lifecycle
    );
    let mut other = scope.clone();
    other.viewport = ViewportId::new("other").unwrap();
    let requests = [
        LocalRequest::FileDrop { instance: id("one"), name: "part.sc".into(), bytes: vec![0; 10] },
        LocalRequest::PointerDrop { instance: id("one"), at: Point { x: 1.0, y: 2.0 }, payload: "fixture".into() },
        LocalRequest::Overlay {
            instance: id("one"),
            region: InputRegion {
                control: ControlId::new("overlay").unwrap(),
                role: InputRole::Overlay,
                bounds: capacity(),
                keyboard_focusable: true,
                accessible_name: "Specimen".into(),
            },
        },
        LocalRequest::Clipboard { instance: id("one"), text: "fixture".into() },
        LocalRequest::Pick { instance: id("one"), at: Point { x: 5.0, y: 5.0 } },
    ];
    for request in requests {
        assert_eq!(local.request(&other, request.clone()).unwrap_err().rule, Rule::Placement);
        let mut wrong_document = scope.clone();
        wrong_document.document = Some(DocumentId::new("wrong-document").unwrap());
        assert_eq!(local.request(&wrong_document, request.clone()).unwrap_err().rule, Rule::Placement);
        local.request(&scope, request).unwrap();
    }
    local.release(&scope, &id("one"));
    assert!(local.keyboard.is_empty());
    assert!(local.requests.is_empty());
}

#[test]
fn reparent_releases_host_local_state_and_retains_transaction() {
    let mut h = PanelMountHarness::new(false).unwrap();
    open_editor(&mut h);
    h.mount(capacity()).unwrap();
    let scope = h.local.scopes.get(&id("editor")).unwrap().clone();
    h.local.request(&scope, LocalRequest::KeyboardOwner { instance: id("editor"), control: ControlId::new("draft").unwrap() }).unwrap();
    h.dispatch(vec![move_editor(Zone::Right)]).unwrap();
    assert!(h.local.keyboard.is_empty());
    assert!(!h.local.scopes.contains_key(&id("editor")));
    assert!(h.probe.state(&editor_key(&h)).unwrap().transaction_live);
}

#[test]
fn reduced_motion_rtl_and_hidden_updates_are_independent_of_paint() {
    let mut h = PanelMountHarness::new(false).unwrap();
    open_editor(&mut h);
    let mut environment = h.runtime.environment().clone();
    environment.direction = Direction::Rtl;
    h.runtime.set_environment(environment, &mut h.local).unwrap();
    let resolved = h.mount(capacity()).unwrap();
    assert!(resolved.panels.first().unwrap().outer.origin.x > 0.0);
    let context = &h.host.mounts.back().unwrap().context;
    assert_eq!(context.direction, Direction::Rtl);
    assert_eq!(context.motion.duration_ms(250), 0);
    assert_eq!(Motion::Animated.duration_ms(600), 500);
    let renders = h.probe.state(&editor_key(&h)).unwrap().renders;
    h.dispatch(vec![Mutation::Collapse { section: section("editor-section"), collapsed: true }]).unwrap();
    for _ in 0..5 {
        h.tick().unwrap();
    }
    assert_eq!(h.probe.state(&editor_key(&h)).unwrap().renders, renders);
    assert!(h.probe.state(&editor_key(&h)).unwrap().updates >= 6);
}

#[test]
fn mount_rejects_duplicate_inactive_or_invalid_regions() {
    let mut h = PanelMountHarness::new(false).unwrap();
    open_editor(&mut h);
    h.tick().unwrap();
    let mut resolved = resolve_geometry(h.runtime.layout(), capacity(), Direction::Ltr, 48.0).unwrap();
    resolved.panels.push(resolved.panels.first().unwrap().clone());
    assert_eq!(
        h.runtime
            .mount(&resolved, &mut h.host, &mut ContentServices { models: &h.models, commands: &mut h.commands, local: &mut h.local })
            .unwrap_err()
            .rule,
        Rule::ExactlyOnce
    );
    assert_eq!(h.host.slot_calls, 0);
    resolved.panels.pop();
    resolved.panels.first_mut().unwrap().body.as_mut().unwrap().size.height = f32::INFINITY;
    assert_eq!(
        h.runtime
            .mount(&resolved, &mut h.host, &mut ContentServices { models: &h.models, commands: &mut h.commands, local: &mut h.local })
            .unwrap_err()
            .rule,
        Rule::Bounds
    );
}

#[test]
fn shutdown_is_atomic_and_explicit_owner_destruction_does_not_reset_layout() {
    let mut h = PanelMountHarness::new(false).unwrap();
    open_editor(&mut h);
    h.tick().unwrap();
    let key = editor_key(&h);
    let layout = h.runtime.layout().layout().clone();
    h.probe.edit(&key, |s| s.reject_cancel = true).unwrap();
    assert_eq!(h.runtime.shutdown(&mut h.local).unwrap_err().rule, Rule::Lifecycle);
    assert_eq!(h.runtime.controller_count(), 1);
    assert_eq!(h.probe.state(&key).unwrap().destroys, 0);
    h.probe.edit(&key, |s| s.reject_cancel = false).unwrap();
    h.runtime.destroy_owner(&key.owner, &mut h.local).unwrap();
    assert_eq!(h.runtime.controller_count(), 0);
    assert_eq!(h.probe.state(&key).unwrap().destroys, 1);
    assert_eq!(h.runtime.layout().layout(), &layout);
    h.tick().unwrap();
    assert_eq!(h.runtime.controller_count(), 0, "destroyed owners cannot silently return");
    h.runtime.revive_owner(&key.owner).unwrap();
    h.tick().unwrap();
    assert_eq!(h.runtime.controller_count(), 1);
}

#[test]
fn shutdown_restored_windows_with_in_app_adapter_releases_controllers_and_local_services() {
    let mut h = restored_window_shutdown_fixture(false);
    let key = editor_key(&h);
    let layout = h.runtime.layout().inspect();
    let releases = h.local.releases;
    let scopes = h.local.scopes.clone();
    let keyboard = h.local.keyboard.clone();
    let requests = h.local.requests.clone();
    let mut native = InAppNativeAdapter;
    assert!(!native.capabilities().native_windows);
    h.probe.edit(&key, |state| state.reject_cancel = true).unwrap();
    assert_eq!(h.runtime.shutdown_with_native(&mut native, &mut h.local).unwrap_err().rule, Rule::Lifecycle);
    assert_eq!(h.runtime.controller_count(), 2);
    assert!(h.runtime.require_live().is_ok());
    assert_eq!(h.runtime.layout().inspect(), layout);
    assert_eq!(h.local.releases, releases);
    assert_eq!(h.local.scopes, scopes);
    assert_eq!(h.local.keyboard, keyboard);
    assert_eq!(h.local.requests, requests);
    assert_eq!(h.probe.state(&key).unwrap().destroys, 0);
    assert!(h.probe.state(&key).unwrap().transaction_live);

    h.probe.edit(&key, |state| state.reject_cancel = false).unwrap();
    h.runtime.shutdown_with_native(&mut native, &mut h.local).unwrap();
    assert_eq!(h.runtime.controller_count(), 0);
    assert_eq!(h.runtime.require_live().unwrap_err().rule, Rule::Lifecycle);
    assert_eq!(h.runtime.layout().inspect(), layout);
    assert_eq!(h.local.releases, releases + 2);
    assert!(h.local.scopes.is_empty());
    assert!(h.local.keyboard.is_empty());
    assert!(h.local.requests.is_empty());
    let log = ControllerKey { instance: id("log"), owner: ContentOwner::Workspace(h.runtime.environment().workspace.clone()) };
    for controller in [&key, &log] {
        let state = h.probe.state(controller).unwrap();
        assert_eq!(state.destroys, 1);
        assert!(state.last_state.unwrap().destroyed);
    }
    assert_eq!(h.probe.state(&key).unwrap().cancels, 1);
    assert!(!h.probe.state(&key).unwrap().transaction_live);
    h.runtime.shutdown_with_native(&mut native, &mut h.local).unwrap();
    assert_eq!(h.local.releases, releases + 2, "repeated shutdown is inert");
    assert_eq!(h.probe.state(&key).unwrap().destroys, 1);
}

#[test]
fn shutdown_native_release_refusal_preserves_controllers_local_services_and_hosts() {
    let mut h = restored_window_shutdown_fixture(true);
    let effects: Vec<_> = h
        .runtime
        .layout()
        .layout()
        .sections
        .iter()
        .map(|section| {
            let SectionLocation::Window { host, .. } = &section.location else { panic!("restored fixture must contain only native placements") };
            HostEffect::CreateNative { host: host.clone(), section: section.id.clone() }
        })
        .collect();
    // Synthetic adapter ownership only; no OS windows are created by this test.
    h.native.prepare(&effects).unwrap();
    h.native.commit(&effects);
    assert_eq!(h.native.hosts.len(), 2);
    let key = editor_key(&h);
    let layout = h.runtime.layout().inspect();
    let hosts = h.native.hosts.clone();
    let commits = h.native.commits;
    let releases = h.local.releases;
    let scopes = h.local.scopes.clone();
    let keyboard = h.local.keyboard.clone();
    let requests = h.local.requests.clone();
    h.native.fail_prepare = true;
    h.probe.edit(&key, |state| state.reject_cancel = true).unwrap();
    assert_eq!(h.runtime.shutdown_with_native(&mut h.native, &mut h.local).unwrap_err().rule, Rule::Lifecycle);
    h.probe.edit(&key, |state| state.reject_cancel = false).unwrap();
    assert_eq!(h.runtime.shutdown_with_native(&mut h.native, &mut h.local).unwrap_err().rule, Rule::Capability);
    assert_eq!(h.runtime.controller_count(), 2);
    assert!(h.runtime.require_live().is_ok());
    assert_eq!(h.runtime.layout().inspect(), layout);
    assert_eq!(h.native.hosts, hosts);
    assert_eq!(h.native.commits, commits);
    assert_eq!(h.local.releases, releases);
    assert_eq!(h.local.scopes, scopes);
    assert_eq!(h.local.keyboard, keyboard);
    assert_eq!(h.local.requests, requests);
    assert_eq!(h.probe.state(&key).unwrap().destroys, 0);
    assert!(h.probe.state(&key).unwrap().transaction_live);

    h.native.fail_prepare = false;
    h.runtime.shutdown_with_native(&mut h.native, &mut h.local).unwrap();
    assert!(h.native.hosts.is_empty());
    assert_eq!(h.native.commits, commits + 1);
    assert_eq!(h.runtime.controller_count(), 0);
    assert_eq!(h.runtime.require_live().unwrap_err().rule, Rule::Lifecycle);
    assert_eq!(h.runtime.layout().inspect(), layout);
    assert_eq!(h.local.releases, releases + 2);
    assert!(h.local.scopes.is_empty());
    assert!(h.local.keyboard.is_empty());
    assert!(h.local.requests.is_empty());
    assert_eq!(h.probe.state(&key).unwrap().destroys, 1);
    assert_eq!(h.probe.state(&key).unwrap().cancels, 1);
    let log = ControllerKey { instance: id("log"), owner: ContentOwner::Workspace(h.runtime.environment().workspace.clone()) };
    assert_eq!(h.probe.state(&log).unwrap().destroys, 1);
    h.runtime.shutdown_with_native(&mut h.native, &mut h.local).unwrap();
    assert_eq!(h.native.commits, commits + 1);
    assert_eq!(h.local.releases, releases + 2);
}

fn restored_window_shutdown_fixture(native: bool) -> PanelMountHarness {
    let mut source = PanelMountHarness::new(true).unwrap();
    open_editor(&mut source);
    source.dispatch(vec![fixture_open("fixture.log", "log", "log-section", Zone::Right, 0).unwrap()]).unwrap();
    let saved = source
        .runtime
        .layout()
        .project(transaction(vec![
            native_placement("editor", "native-editor", "editor-host", 0),
            native_placement("log", "native-log", "log-host", 1),
        ]))
        .unwrap();
    let registry = source.runtime.layout().registry().clone();
    let bytes = encode(&LayoutEnvelope::new(saved.clone()), &registry).unwrap();
    let restored = decode(&bytes, &registry, source.runtime.layout().layout()).unwrap();
    assert!(restored.notice.is_none());
    assert_eq!(restored.envelope.layout, saved);
    let mut h = PanelMountHarness::without_content(native).unwrap();
    h.runtime =
        PanelRuntime::new(LayoutService::new(registry, restored.envelope.layout, h.native.capabilities()).unwrap(), h.runtime.environment().clone())
            .unwrap();
    bind_synthetic_controllers(&mut h.runtime, &h.probe).unwrap();
    h.tick().unwrap();
    assert_eq!(h.runtime.controller_count(), 2);
    for (instance, host) in [("editor", "editor-host"), ("log", "log-host")] {
        let mut scope = runtime_scope(&h.runtime);
        scope.viewport = ViewportId::new(host).unwrap();
        h.local.bind(id(instance), scope.clone()).unwrap();
        h.local.request(&scope, LocalRequest::KeyboardOwner { instance: id(instance), control: ControlId::new("draft").unwrap() }).unwrap();
    }
    h
}

#[test]
fn runtime_action_sink_and_native_service_scopes_use_the_shared_interfaces() {
    let mut h = PanelMountHarness::new(true).unwrap();
    {
        let mut services = ContentServices { models: &h.models, commands: &mut h.commands, local: &mut h.local };
        let mut adapter = RuntimeActionSink { runtime: &mut h.runtime, native: &mut h.native, services: &mut services };
        let sink: &mut dyn ActionSink = &mut adapter;
        sink.dispatch(transaction(vec![fixture_open("fixture.log", "one", "first", Zone::Left, 0).unwrap()])).unwrap();
        assert_eq!(sink.inspect().layout.instances.len(), 1);
    }
    h.dispatch(vec![Mutation::Move {
        instance: id("one"),
        destination: Destination::NewSection {
            section: section("native"),
            index: 0,
            location: SectionLocation::Window {
                host: HostId::new("native-viewport").unwrap(),
                origin: Point { x: 0.0, y: 0.0 },
                return_to: DockReturn {
                    zone: Zone::Left,
                    index: 0,
                    size: Size { width: 280.0, height: 240.0 },
                    collapsed: false,
                    join: None,
                    section: None,
                    column: None,
                    column_index: 0,
                    column_width: None,
                    weight: 1.0,
                    preferred_extent: None,
                    extent_priority: 0,
                },
            },
            size: Size { width: 280.0, height: 240.0 },
        },
    }])
    .unwrap();
    h.mount(capacity()).unwrap();
    assert_eq!(h.host.mounts.back().unwrap().context.viewport, ViewportId::new("native-viewport").unwrap());
    let scope = h.local.scopes.get(&id("one")).unwrap().clone();
    assert_eq!(scope.viewport, ViewportId::new("native-viewport").unwrap());
    let mut root = scope.clone();
    root.viewport = h.runtime.environment().viewport.clone();
    assert_eq!(h.local.request(&root, LocalRequest::Clipboard { instance: id("one"), text: "fixture".into() }).unwrap_err().rule, Rule::Placement);
}

#[test]
fn egui_host_uses_logical_bounds_at_two_hundred_percent_scale() {
    let mut h = PanelMountHarness::new(false).unwrap();
    open_editor(&mut h);
    h.tick().unwrap();
    h.dispatch(vec![Mutation::SetSize { section: section("editor-section"), size: Size { width: 160.0, height: 120.0 } }]).unwrap();
    let resolved = resolve_geometry(h.runtime.layout(), capacity(), Direction::Ltr, 48.0).unwrap();
    let ctx = egui::Context::default();
    ctx.set_pixels_per_point(2.0);
    let mut failure = None;
    let mut output = ctx.run_ui(egui::RawInput::default(), |root_ui| {
        crate::ui_kit::Container::new(root_ui.ctx(), crate::ui_kit::Surface::Viewport).central().show(root_ui, |ui| {
            let mut host = EguiPanelHost { ui };
            if let Err(reason) =
                h.runtime.mount(&resolved, &mut host, &mut ContentServices { models: &h.models, commands: &mut h.commands, local: &mut h.local })
            {
                failure = Some(reason);
            }
        });
    });
    output.textures_delta.clear();
    assert!(failure.is_none(), "{failure:?}");
    assert_eq!(resolved.panels.first().unwrap().outer.size, Size { width: 160.0, height: 120.0 });
    assert!(h.probe.state(&editor_key(&h)).unwrap().renders >= 1);
}

struct AppFixtureServices {
    models: FakeModels,
    commands: FakeContentCommands,
    local: FakeLocalServices,
    refreshed: u64,
}
impl AppPanelServices for AppFixtureServices {
    fn refresh_read_only(&mut self, _session: &solidcraft_engine::Session) -> PanelResult<()> {
        self.refreshed += 1;
        Ok(())
    }
    fn services(&mut self) -> ContentServices<'_> {
        ContentServices { models: &self.models, commands: &mut self.commands, local: &mut self.local }
    }
}

#[test]
fn solve_app_integration_is_opt_in_and_cannot_dirty_or_mutate_document() {
    let mut app = crate::SolveApp::new(solidcraft_engine::Session::default(), Default::default());
    assert!(app.panel_runtime().is_none());
    let doc = app.session.doc.clone();
    let model = app.session.model.state();
    let revision = app.session.revision;
    let undo = app.session.undo.len();
    let dirty = app.session.is_dirty();
    let recomputed = app.session.model.recomputed;
    let mut h = PanelMountHarness::new(false).unwrap();
    open_editor(&mut h);
    app.enable_panel_runtime(
        h.runtime,
        Box::new(AppFixtureServices {
            models: FakeModels { editable: true },
            commands: FakeContentCommands::default(),
            local: FakeLocalServices::default(),
            refreshed: 0,
        }),
    )
    .unwrap();
    super::app::tick_app(&mut app, &egui::Context::default());
    assert_eq!(app.panel_runtime().unwrap().controller_count(), 2, "the synthetic and real document retain separate controller owners");
    assert!(std::sync::Arc::ptr_eq(&doc, &app.session.doc));
    assert!(std::sync::Arc::ptr_eq(&model, &app.session.model.state()));
    assert_eq!(app.session.revision, revision);
    assert_eq!(app.session.undo.len(), undo);
    assert_eq!(app.session.is_dirty(), dirty);
    assert_eq!(app.session.model.recomputed, recomputed);
    app.disable_panel_runtime().unwrap();
    assert!(app.panel_runtime().is_none());
}

#[test]
fn saved_layout_hiding_and_workspace_switches_retain_all_content_owners() {
    let mut h = PanelMountHarness::new(false).unwrap();
    open_editor(&mut h);
    h.dispatch(vec![
        fixture_open("fixture.application", "application", "application-section", Zone::Right, 1).unwrap(),
        fixture_open("fixture.log", "log", "log-section", Zone::Bottom, 2).unwrap(),
    ])
    .unwrap();
    h.mount(capacity()).unwrap();
    let saved = h.runtime.layout().layout().clone();
    let key = editor_key(&h);
    let app_key = ControllerKey { instance: id("application"), owner: ContentOwner::Application };
    let log_key = ControllerKey { instance: id("log"), owner: ContentOwner::Workspace(saved.workspace.clone()) };
    for (key, draft) in [(&key, "active editor draft"), (&app_key, "application conversation"), (&log_key, "workspace conversation")] {
        h.probe.edit(key, |s| s.draft = draft.into()).unwrap();
    }
    h.probe
        .edit(&key, |s| {
            s.reject_cancel = true;
            s.reject_reopen = true;
        })
        .unwrap();
    let updates = h.probe.state(&key).unwrap().updates;
    let hidden = Layout::empty(saved.workspace.clone());
    h.apply_layout(hidden, None).unwrap();
    let resolved = h.mount(capacity()).unwrap();
    assert!(resolved.panels.is_empty());
    assert!(resolved.rails.is_empty());
    assert!(!h.runtime.state(&key).unwrap().presented);
    assert!(h.probe.state(&key).unwrap().transaction_live);
    assert!(h.probe.state(&key).unwrap().updates > updates);
    assert_eq!(h.probe.state(&key).unwrap().cancels, 0);
    let mut other = saved.clone();
    other.workspace = WorkspaceId::new("workspace-two").unwrap();
    h.apply_layout(other.clone(), None).unwrap();
    h.tick().unwrap();
    let other_log = ControllerKey { instance: id("log"), owner: ContentOwner::Workspace(other.workspace.clone()) };
    assert_eq!(h.runtime.controller_count(), 4);
    assert!(h.runtime.state(&other_log).is_some());
    assert!(!h.runtime.state(&log_key).unwrap().presented);
    let hidden_updates = h.probe.state(&log_key).unwrap().updates;
    h.tick().unwrap();
    assert!(h.probe.state(&log_key).unwrap().updates > hidden_updates);
    h.probe.edit(&other_log, |s| s.draft = "other workspace draft".into()).unwrap();
    h.apply_layout(saved.clone(), None).unwrap();
    h.mount(capacity()).unwrap();
    assert_eq!(h.runtime.layout().layout(), &saved);
    assert_eq!(h.runtime.environment().workspace, saved.workspace);
    assert_eq!(h.runtime.controller_count(), 4);
    assert!(h.runtime.state(&key).unwrap().presented);
    for (key, draft) in [
        (&key, "active editor draft"),
        (&app_key, "application conversation"),
        (&log_key, "workspace conversation"),
        (&other_log, "other workspace draft"),
    ] {
        assert_eq!(h.probe.state(key).unwrap().draft, draft);
    }
    assert_eq!((h.probe.state(&key).unwrap().cancels, h.commands.calls), (0, 0));
    assert_eq!(h.probe.state(&key).unwrap().reopens, 0);
}

#[test]
fn failed_layout_application_preserves_environment_controllers_native_and_local_state() {
    let mut h = PanelMountHarness::new(true).unwrap();
    open_editor(&mut h);
    h.mount(capacity()).unwrap();
    let scope = h.local.scopes[&id("editor")].clone();
    h.local.request(&scope, LocalRequest::KeyboardOwner { instance: id("editor"), control: ControlId::new("draft").unwrap() }).unwrap();
    let before = h.runtime.layout().inspect();
    let environment = h.runtime.environment().clone();
    let controllers = h.runtime.controller_count();
    let keyboard = h.local.keyboard.clone();
    let scopes = h.local.scopes.clone();
    let releases = h.local.releases;
    let commits = h.native.commits;
    let mut other = before.layout.clone();
    other.workspace = WorkspaceId::new("other-workspace").unwrap();
    h.native.fail_prepare = true;
    assert_eq!(h.apply_layout(other.clone(), Some(before.revision)).unwrap_err().rule, Rule::Capability);
    assert_eq!(h.runtime.layout().inspect(), before);
    assert_eq!(h.runtime.environment(), &environment);
    assert_eq!(h.runtime.controller_count(), controllers);
    assert_eq!(h.local.keyboard, keyboard);
    assert_eq!(h.local.scopes, scopes);
    assert_eq!(h.local.releases, releases);
    assert_eq!(h.native.commits, commits);
    h.native.fail_prepare = false;
    assert_eq!(h.apply_layout(other.clone(), Some(before.revision + 1)).unwrap_err().rule, Rule::Revision);
    other.instances.first_mut().unwrap().kind = PanelKind::new("fixture.log").unwrap();
    assert_eq!(h.apply_layout(other, None).unwrap_err().rule, Rule::DuplicateId);
    assert_eq!(h.runtime.layout().inspect(), before);
    assert_eq!(h.local.keyboard, keyboard);
}

#[test]
fn snapshot_application_keeps_missing_bindings_and_unavailable_content_dormant() {
    let mut source = PanelMountHarness::new(false).unwrap();
    open_editor(&mut source);
    let saved = source.runtime.layout().layout().clone();
    let mut h = PanelMountHarness::without_content(false).unwrap();
    h.apply_layout(saved.clone(), None).unwrap();
    assert!(matches!(h.runtime.layout().availability(&id("editor")), Availability::Unavailable { .. }));
    assert_eq!(h.runtime.controller_count(), 0);
    assert_eq!(h.runtime.layout().layout(), &saved);
    assert_eq!(h.dispatch(vec![Mutation::Focus { instance: id("editor") }]).unwrap_err().rule, Rule::Availability);
    let mut contextual = PanelMountHarness::new(false).unwrap();
    contextual.models.editable = false;
    contextual.apply_layout(saved.clone(), None).unwrap();
    contextual.tick().unwrap();
    assert_eq!(contextual.runtime.layout().layout(), &saved);
    assert!(!contextual.runtime.state(&editor_key(&contextual)).unwrap().presented);
    contextual.models.editable = true;
    contextual.mount(capacity()).unwrap();
    assert!(contextual.runtime.state(&editor_key(&contextual)).unwrap().presented);
}

#[test]
fn explicit_lifecycle_intents_cannot_be_erased_by_a_batch() {
    let mut h = PanelMountHarness::new(true).unwrap();
    open_editor(&mut h);
    h.mount(capacity()).unwrap();
    let key = editor_key(&h);
    h.probe.edit(&key, |s| s.reject_cancel = true).unwrap();
    let scope = h.local.scopes[&id("editor")].clone();
    h.local.request(&scope, LocalRequest::KeyboardOwner { instance: id("editor"), control: ControlId::new("draft").unwrap() }).unwrap();
    let reopen = Mutation::Open { kind: PanelKind::new("fixture.editor").unwrap(), instance: id("editor"), destination: None };
    let before = h.runtime.layout().inspect();
    let keyboard = h.local.keyboard.clone();
    let scopes = h.local.scopes.clone();
    let releases = h.local.releases;
    let commits = h.native.commits;
    for batch in [
        vec![Mutation::Close { instance: id("editor") }, reopen.clone()],
        vec![Mutation::Close { instance: id("editor") }, move_editor(Zone::Right), reopen.clone()],
        vec![Mutation::Destroy { instance: id("editor") }, reopen],
    ] {
        assert_eq!(h.dispatch(batch).unwrap_err().rule, Rule::Lifecycle);
        assert_eq!(h.runtime.layout().inspect(), before);
        assert_eq!(h.local.keyboard, keyboard);
        assert_eq!(h.local.scopes, scopes);
        assert_eq!(h.local.releases, releases);
        assert_eq!(h.native.commits, commits);
        assert!(h.probe.state(&key).unwrap().transaction_live);
        assert_eq!(h.probe.state(&key).unwrap().cancels, 0);
    }
    h.dispatch(vec![
        Mutation::SetSize { section: section("editor-section"), size: Size { width: 300.0, height: 260.0 } },
        fixture_open("fixture.log", "independent", "independent-section", Zone::Right, 1).unwrap(),
    ])
    .unwrap();
    assert_eq!(h.runtime.controller_count(), 2);
    // Noncanceling snapshot hiding is distinct from an explicit Close, including
    // when the saved entry is already marked closed.
    let mut hidden = h.runtime.layout().layout().clone();
    hidden.sections.retain(|s| s.id != section("editor-section"));
    hidden.instances.iter_mut().find(|p| p.id == id("editor")).unwrap().requested_open = false;
    h.apply_layout(hidden, None).unwrap();
    assert_eq!(h.dispatch(vec![Mutation::Close { instance: id("editor") }]).unwrap_err().rule, Rule::Lifecycle);
}

#[test]
fn service_errors_do_not_starve_later_hidden_controllers_or_leave_stale_input() {
    let mut h = PanelMountHarness::new(false).unwrap();
    open_editor(&mut h);
    h.dispatch(vec![fixture_open("fixture.log", "zz-log", "log-section", Zone::Right, 1).unwrap()]).unwrap();
    h.mount(capacity()).unwrap();
    let editor = editor_key(&h);
    let log = ControllerKey { instance: id("zz-log"), owner: ContentOwner::Workspace(h.runtime.environment().workspace.clone()) };
    let mut layout = h.runtime.layout().layout().clone();
    layout.sections.retain(|s| s.id != section("log-section"));
    layout.instances.retain(|p| p.id != id("zz-log"));
    h.apply_layout(layout, None).unwrap();
    h.mount(capacity()).unwrap();
    let scope = h.local.scopes[&id("editor")].clone();
    h.local.request(&scope, LocalRequest::KeyboardOwner { instance: id("editor"), control: ControlId::new("draft").unwrap() }).unwrap();
    h.probe.edit(&editor, |s| s.fail_update = true).unwrap();
    let updates = h.probe.state(&log).unwrap().updates;
    for _ in 0..4 {
        assert!(h.tick().is_err());
        assert_eq!(h.runtime.update_errors().len(), 1);
        assert_eq!(h.runtime.update_errors().first().unwrap().controller.as_ref(), Some(&editor));
    }
    assert_eq!(h.probe.state(&log).unwrap().updates, updates + 4);
    assert!(!h.runtime.state(&log).unwrap().presented);
    assert!(!h.runtime.state(&editor).unwrap().presented);
    assert!(h.local.keyboard.is_empty());
    assert!(matches!(h.runtime.layout().availability(&id("editor")), Availability::Unavailable { .. }));
    h.probe
        .edit(&editor, |s| {
            s.fail_update = false;
            s.invalid_availability = true;
        })
        .unwrap();
    assert!(h.tick().is_err());
    assert_eq!(h.runtime.update_errors().first().unwrap().error.rule, Rule::Availability);
    assert_eq!(h.probe.state(&log).unwrap().updates, updates + 5);
    h.probe.edit(&editor, |s| s.invalid_availability = false).unwrap();
    h.tick().unwrap();
    assert!(h.runtime.update_errors().is_empty());
    h.mount(capacity()).unwrap();
    assert!(h.runtime.state(&editor).unwrap().presented);
}

#[test]
fn snapshot_native_transitions_stage_effects_without_canceling_editors() {
    let mut h = PanelMountHarness::new(true).unwrap();
    open_editor(&mut h);
    h.tick().unwrap();
    let key = editor_key(&h);
    h.probe.edit(&key, |s| s.reject_cancel = true).unwrap();
    let location = SectionLocation::Window {
        host: HostId::new("snapshot-host").unwrap(),
        origin: Point { x: 0.0, y: 0.0 },
        return_to: DockReturn {
            zone: Zone::Left,
            index: 0,
            size: Size { width: 280.0, height: 240.0 },
            collapsed: false,
            join: None,
            section: None,
            column: None,
            column_index: 0,
            column_width: None,
            weight: 1.0,
            preferred_extent: None,
            extent_priority: 0,
        },
    };
    let native_layout = h
        .runtime
        .layout()
        .project(transaction(vec![Mutation::Move {
            instance: id("editor"),
            destination: Destination::NewSection {
                section: section("editor-section"),
                index: 0,
                location,
                size: Size { width: 280.0, height: 240.0 },
            },
        }]))
        .unwrap();
    assert!(native_layout.sections[0].column.is_none());
    let SectionLocation::Window { return_to, .. } = &native_layout.sections[0].location else {
        panic!("projected native fixture must retain its dock return");
    };
    assert_eq!(return_to.column, h.runtime.layout().layout().sections[0].column);
    assert!(return_to.column.is_some());
    let reply = h.apply_layout(native_layout.clone(), None).unwrap();
    assert!(reply.effects.iter().any(|e| matches!(e, HostEffect::CreateNative { .. })));
    assert_eq!(h.native.hosts.len(), 1);
    let away = Layout::empty(WorkspaceId::new("away").unwrap());
    let before = h.runtime.layout().inspect();
    let hosts = h.native.hosts.clone();
    h.native.fail_prepare = true;
    assert_eq!(h.apply_layout(away.clone(), None).unwrap_err().rule, Rule::Capability);
    assert_eq!(h.runtime.layout().inspect(), before);
    assert_eq!(h.native.hosts, hosts);
    h.native.fail_prepare = false;
    let reply = h.apply_layout(away, None).unwrap();
    assert!(reply.effects.iter().any(|e| matches!(e, HostEffect::ReleaseNative { .. })));
    assert!(h.native.hosts.is_empty());
    h.apply_layout(native_layout, None).unwrap();
    assert_eq!(h.native.hosts.len(), 1);
    assert_eq!(h.runtime.controller_count(), 1);
    assert!(h.probe.state(&key).unwrap().transaction_live);
    assert_eq!(h.probe.state(&key).unwrap().cancels, 0);
}

#[test]
fn egui_reference_host_rejects_sparse_or_no_input_geometry_before_widget_work() {
    for mode in 0..3 {
        let mut h = PanelMountHarness::new(false).unwrap();
        open_editor(&mut h);
        h.tick().unwrap();
        let mut resolved = resolve_geometry(h.runtime.layout(), capacity(), Direction::Ltr, 48.0).unwrap();
        let geometry = resolved.panels.first_mut().unwrap();
        let island = LogicalRect { origin: geometry.outer.origin, size: Size { width: 20.0, height: 20.0 } };
        match mode {
            0 => geometry.input.clear(),
            1 => {
                geometry.painted = vec![island];
                geometry.input.clear();
            }
            _ => geometry.input.first_mut().unwrap().bounds = island,
        }
        resolved.validate().unwrap();
        let ctx = egui::Context::default();
        let mut failure = None;
        let mut output = ctx.run_ui(egui::RawInput::default(), |root_ui| {
            crate::ui_kit::Container::new(root_ui.ctx(), crate::ui_kit::Surface::Viewport).central().show(root_ui, |ui| {
                let before = ui.min_rect();
                let mut host = EguiPanelHost { ui };
                failure = h
                    .runtime
                    .mount(&resolved, &mut host, &mut ContentServices { models: &h.models, commands: &mut h.commands, local: &mut h.local })
                    .err();
                assert_eq!(host.ui.min_rect(), before);
            });
        });
        output.textures_delta.clear();
        assert_eq!(failure.unwrap().rule, Rule::Capability);
        assert_eq!(h.probe.state(&editor_key(&h)).unwrap().renders, 0);
    }
}

#[test]
fn normal_egui_mount_preserves_the_parent_clip() {
    let mut h = PanelMountHarness::new(false).unwrap();
    open_editor(&mut h);
    h.tick().unwrap();
    let resolved = resolve_geometry(h.runtime.layout(), capacity(), Direction::Ltr, 48.0).unwrap();
    let ctx = egui::Context::default();
    let parent = egui::Rect::from_min_max(egui::pos2(30.0, 40.0), egui::pos2(150.0, 160.0));
    let mut failure = None;
    let mut output = ctx.run_ui(egui::RawInput::default(), |root_ui| {
        crate::ui_kit::Container::new(root_ui.ctx(), crate::ui_kit::Surface::Viewport).central().show(root_ui, |ui| {
            ui.set_clip_rect(ui.clip_rect().intersect(parent));
            let mut host = EguiPanelHost { ui };
            failure = h
                .runtime
                .mount(&resolved, &mut host, &mut ContentServices { models: &h.models, commands: &mut h.commands, local: &mut h.local })
                .err();
        });
    });
    output.textures_delta.clear();
    assert!(failure.is_none(), "{failure:?}");
    let clip = h.probe.state(&editor_key(&h)).unwrap().last_clip.unwrap();
    assert!(parent.contains_rect(clip));
    assert!(h.probe.state(&editor_key(&h)).unwrap().renders >= 1);
}

#[derive(Default)]
struct ForwardingTrace {
    service_borrows: usize,
    refreshes: usize,
    content_calls: u64,
}
struct ForwardingAppServices {
    models: FakeModels,
    commands: FakeContentCommands,
    local: FakeLocalServices,
    trace: std::rc::Rc<std::cell::RefCell<ForwardingTrace>>,
}

fn panel_command_editor() -> serde_json::Value {
    serde_json::json!({"kind":"fixture.editor","instance":"editor","destination":{"type":"new_section","section":"editor-section","index":0,
        "location":{"type":"dock","zone":"left"},"size":{"width":280,"height":240}}})
}
fn panel_command_editor_target() -> serde_json::Value {
    serde_json::json!({"type":"instance","id":"editor"})
}
fn panel_command_native_move() -> serde_json::Value {
    serde_json::json!({"target":panel_command_editor_target(),"destination":{"type":"new_section","section":"native","index":0,
        "location":{"type":"window","host":"native-host","origin":{"x":10,"y":20},"return_to":{"zone":"left","index":0,"size":{"width":280,"height":240},"collapsed":false}},
        "size":{"width":280,"height":240}}})
}

fn prepare_app_commands(app: &mut crate::SolveApp) {
    if !app.universal_panel_host_enabled() {
        app.enable_universal_panel_host(HostOptions::default()).unwrap();
    }
    app.panel_runtime.as_mut().unwrap().prepare_host(&app.session, capacity()).unwrap();
}

#[test]
fn application_structural_commands_require_a_captured_host_environment() {
    let PanelMountHarness { runtime, .. } = PanelMountHarness::new(false).unwrap();
    let mut app = crate::SolveApp::new(solidcraft_engine::Session::default(), Default::default());
    app.enable_panel_runtime(runtime, Box::new(FakeAppPanelServices { models: FakeModels { editable: true }, ..Default::default() })).unwrap();
    assert_eq!(app.run_panel_command("panel.open", &panel_command_editor()).unwrap_err().rule, Rule::Availability);
    app.enable_universal_panel_host(HostOptions::default()).unwrap();
    assert_eq!(app.run_panel_command("panel.open", &panel_command_editor()).unwrap_err().rule, Rule::Availability);
    prepare_app_commands(&mut app);
    app.run_panel_command("panel.open", &panel_command_editor()).unwrap();
    assert_eq!(app.panel_runtime().unwrap().layout().layout().sections.len(), 1);
}

#[test]
fn panel_commands_use_runtime_guards_and_keep_every_authoritative_state_on_preflight_failure() {
    use commands::LayoutCatalogue;
    let mut h = PanelMountHarness::new(true).unwrap();
    h.command("panel.open", &panel_command_editor()).unwrap();
    h.mount(capacity()).unwrap();
    let key = editor_key(&h);
    h.probe
        .edit(&key, |state| {
            state.draft = "unsaved command draft".into();
            state.reject_cancel = true;
        })
        .unwrap();
    let before = h.runtime.layout().inspect();
    let catalogue = h.catalogue.state().clone();
    let controllers = h.runtime.controller_count();
    let local_keyboard = h.local.keyboard.clone();
    let local_scopes = h.local.scopes.clone();
    let releases = h.local.releases;
    let requests = h.local.requests.len();
    let hosts = h.native.hosts.clone();
    assert_eq!(h.command("panel.close", &serde_json::json!({"target":panel_command_editor_target()})).unwrap_err().rule, Rule::Lifecycle);
    h.native.fail_prepare = true;
    assert_eq!(h.command("panel.popout", &panel_command_native_move()).unwrap_err().rule, Rule::Capability);
    h.native.fail_prepare = false;
    h.catalogue.fail_stage = true;
    assert_eq!(
        h.command("panel.collapse", &serde_json::json!({"target":panel_command_editor_target(),"collapsed":true})).unwrap_err().rule,
        Rule::Capability
    );
    h.catalogue.fail_stage = false;
    let steps = vec![
        serde_json::json!({"command":"panel.close","params":{"target":panel_command_editor_target()}}),
        serde_json::json!({"command":"panel.open","params":{"kind":"fixture.editor","instance":"editor"}}),
    ];
    assert_eq!(h.command_batch(&steps).unwrap_err().rule, Rule::Lifecycle);
    assert_eq!(h.runtime.layout().inspect(), before);
    assert_eq!(h.catalogue.state(), &catalogue);
    assert_eq!(h.runtime.controller_count(), controllers);
    assert_eq!(h.local.keyboard, local_keyboard);
    assert_eq!(h.native.hosts, hosts);
    assert_eq!(h.local.scopes, local_scopes);
    assert_eq!(h.local.releases, releases);
    assert_eq!(h.local.requests.len(), requests);
    let editor = h.probe.state(&key).unwrap();
    assert_eq!(editor.draft, "unsaved command draft");
    assert!(editor.transaction_live);
    assert_eq!(editor.cancels, 0);
}

#[test]
fn panel_commands_replace_layouts_and_workspaces_on_retained_runtime_with_live_hidden_services() {
    let mut h = PanelMountHarness::new(false).unwrap();
    h.command("panel.open", &panel_command_editor()).unwrap();
    h.tick().unwrap();
    let key = editor_key(&h);
    h.probe
        .edit(&key, |s| {
            s.draft = "retained across commands".into();
            s.reject_cancel = true;
        })
        .unwrap();
    h.command("layout.save", &serde_json::json!({"name":"Draft layout"})).unwrap();
    let updates = h.probe.state(&key).unwrap().updates;
    h.command("layout.reset", &serde_json::json!({})).unwrap();
    h.tick().unwrap();
    assert!(h.probe.state(&key).unwrap().updates > updates);
    h.command("layout.apply", &serde_json::json!({"name":"Draft layout"})).unwrap();
    h.command("layout.workspace.select", &serde_json::json!({"workspace":"fixture.other"})).unwrap();
    let updates = h.probe.state(&key).unwrap().updates;
    h.tick().unwrap();
    assert!(h.probe.state(&key).unwrap().updates > updates);
    h.command("layout.workspace.select", &serde_json::json!({"workspace":"fixture.workspace"})).unwrap();
    h.tick().unwrap();
    assert_eq!(h.runtime.controller_count(), 1);
    let state = h.probe.state(&key).unwrap();
    assert_eq!(state.draft, "retained across commands");
    assert!(state.transaction_live);
    assert_eq!(state.cancels, 0);
    assert_eq!(h.commands.calls, 0, "layout never runs content/model commands");
}

#[test]
fn panel_workspace_commands_reuse_each_workspaces_controller_and_background_draft() {
    let mut h = PanelMountHarness::new(false).unwrap();
    let mut input = panel_command_editor();
    input["kind"] = serde_json::json!("fixture.log");
    input["instance"] = serde_json::json!("log");
    h.command("panel.open", &input).unwrap();
    h.tick().unwrap();
    let a = ControllerKey { instance: id("log"), owner: ContentOwner::Workspace(WorkspaceId::new("fixture.workspace").unwrap()) };
    h.probe.edit(&a, |s| s.draft = "workspace A draft".into()).unwrap();
    h.command("layout.save", &serde_json::json!({"name":"Shared instances"})).unwrap();
    h.command("layout.workspace.select", &serde_json::json!({"workspace":"fixture.other"})).unwrap();
    h.command("layout.apply", &serde_json::json!({"name":"Shared instances"})).unwrap();
    h.tick().unwrap();
    let b = ControllerKey { instance: id("log"), owner: ContentOwner::Workspace(WorkspaceId::new("fixture.other").unwrap()) };
    h.probe.edit(&b, |s| s.draft = "workspace B draft".into()).unwrap();
    let updates = h.probe.state(&a).unwrap().updates;
    h.tick().unwrap();
    assert!(h.probe.state(&a).unwrap().updates > updates);
    h.command("layout.workspace.select", &serde_json::json!({"workspace":"fixture.workspace"})).unwrap();
    h.tick().unwrap();
    assert_eq!(h.probe.state(&a).unwrap().draft, "workspace A draft");
    assert_eq!(h.probe.state(&b).unwrap().draft, "workspace B draft");
    assert_eq!(h.runtime.controller_count(), 2);
    h.command("layout.workspace.select", &serde_json::json!({"workspace":"fixture.other"})).unwrap();
    h.tick().unwrap();
    assert_eq!(h.probe.state(&b).unwrap().draft, "workspace B draft");
    assert_eq!(h.runtime.controller_count(), 2);
}

#[test]
fn panel_native_close_command_redocks_without_cancelling_or_replacing_editor() {
    let mut h = PanelMountHarness::new(true).unwrap();
    h.command("panel.open", &panel_command_editor()).unwrap();
    h.tick().unwrap();
    let key = editor_key(&h);
    h.probe
        .edit(&key, |s| {
            s.reject_cancel = true;
            s.draft = "native draft".into();
        })
        .unwrap();
    h.command("panel.popout", &panel_command_native_move()).unwrap();
    assert_eq!(h.native.hosts.len(), 1);
    let reply = h.command("panel.native_close", &serde_json::json!({"target":{"type":"native_host","id":"native-host"}})).unwrap();
    assert!(h.native.hosts.is_empty());
    assert_eq!(reply["layout"]["sections"][0]["location"]["type"], "dock");
    assert_eq!(h.probe.state(&key).unwrap().draft, "native draft");
    assert_eq!(h.probe.state(&key).unwrap().cancels, 0);
    assert_eq!(h.runtime.controller_count(), 1);
}

#[test]
fn solve_app_panel_commands_use_injected_services_without_document_camera_or_cache_changes() {
    let PanelMountHarness { runtime, probe, .. } = PanelMountHarness::new(false).unwrap();
    let trace = std::rc::Rc::new(std::cell::RefCell::new(ForwardingTrace::default()));
    let mut app = crate::SolveApp::new(solidcraft_engine::Session::default(), Default::default());
    let revision = app.session.revision;
    let doc = app.session.doc.clone();
    let model = app.session.model.state();
    let undo = app.session.undo.len();
    let dirty = app.session.is_dirty();
    let camera = app.cam;
    app.enable_panel_runtime(
        runtime,
        Box::new(ForwardingAppServices {
            models: FakeModels { editable: true },
            commands: Default::default(),
            local: Default::default(),
            trace: trace.clone(),
        }),
    )
    .unwrap();
    let catalogue = fixture_command_catalogue(app.panel_runtime().unwrap().layout().registry()).unwrap();
    app.panel_runtime_mut().unwrap().set_command_adapters(Box::new(catalogue), Box::new(FakeNativeAdapter::new(false))).unwrap();
    prepare_app_commands(&mut app);
    app.run("panel.open", panel_command_editor()).unwrap();
    let key = ControllerKey { instance: id("editor"), owner: ContentOwner::Document(DocumentId::new(app.session.document_id.clone()).unwrap()) };
    probe.edit(&key, |s| s.draft = "app command draft".into()).unwrap();
    app.run("layout.save", serde_json::json!({"name":"App saved"})).unwrap();
    app.run("layout.reset", serde_json::json!({})).unwrap();
    app.tick_panel_runtime().unwrap();
    app.run("layout.apply", serde_json::json!({"name":"App saved"})).unwrap();
    assert_eq!(app.run("panel.list", serde_json::json!({})).unwrap()["state"]["layout"]["instances"][0]["id"], "editor");
    assert_eq!(probe.state(&key).unwrap().draft, "app command draft");
    assert_eq!(probe.state(&key).unwrap().cancels, 0);
    assert!(trace.borrow().service_borrows >= 6);
    assert_eq!(app.session.revision, revision);
    assert_eq!(app.session.undo.len(), undo);
    assert_eq!(app.session.is_dirty(), dirty);
    assert_eq!(app.cam, camera);
    assert!(std::sync::Arc::ptr_eq(&doc, &app.session.doc));
    assert!(std::sync::Arc::ptr_eq(&model, &app.session.model.state()));
}
impl AppPanelServices for ForwardingAppServices {
    fn refresh_read_only(&mut self, _session: &solidcraft_engine::Session) -> PanelResult<()> {
        let mut trace = self.trace.try_borrow_mut().map_err(|_| error(Rule::Lifecycle, "trace borrowed"))?;
        trace.refreshes += 1;
        trace.content_calls = self.commands.calls;
        Ok(())
    }
    fn services(&mut self) -> ContentServices<'_> {
        if let Ok(mut trace) = self.trace.try_borrow_mut() {
            trace.service_borrows += 1;
        }
        ContentServices { models: &self.models, commands: &mut self.commands, local: &mut self.local }
    }
}

#[test]
fn solve_app_forwards_dispatch_mount_content_switch_and_ticks_to_its_injected_provider() {
    let PanelMountHarness { runtime, probe, .. } = PanelMountHarness::new(false).unwrap();
    let catalogue = fixture_command_catalogue(runtime.layout().registry()).unwrap();
    let trace = std::rc::Rc::new(std::cell::RefCell::new(ForwardingTrace::default()));
    let mut app = crate::SolveApp::new(solidcraft_engine::Session::default(), Default::default());
    let revision = app.session.revision;
    let doc = app.session.doc.clone();
    let model = app.session.model.state();
    let undo = app.session.undo.len();
    let dirty = app.session.is_dirty();
    app.enable_panel_runtime(
        runtime,
        Box::new(ForwardingAppServices {
            models: FakeModels { editable: true },
            commands: FakeContentCommands::default(),
            local: FakeLocalServices::default(),
            trace: trace.clone(),
        }),
    )
    .unwrap();
    app.panel_runtime_mut().unwrap().set_command_adapters(Box::new(catalogue), Box::new(InAppNativeAdapter)).unwrap();
    prepare_app_commands(&mut app);
    app.dispatch_panel_layout(transaction(vec![
        fixture_open("fixture.editor", "editor", "editor-section", Zone::Left, 0).unwrap(),
        fixture_open("fixture.log", "log", "log-section", Zone::Right, 1).unwrap(),
    ]))
    .unwrap();
    app.tick_panel_runtime().unwrap();
    let key = ControllerKey { instance: id("editor"), owner: ContentOwner::Document(DocumentId::new(app.session.document_id.clone()).unwrap()) };
    probe.edit(&key, |state| state.draft = "draft through app".into()).unwrap();
    let saved = app.panel_runtime().unwrap().layout().layout().clone();
    let resolved = resolve_geometry(app.panel_runtime().unwrap().layout(), capacity(), Direction::Ltr, 48.0).unwrap();
    let mut host = FakePanelHost::default();
    app.disable_universal_panel_host().unwrap();
    app.mount_panels(&resolved, &mut host).unwrap();
    prepare_app_commands(&mut app);
    assert!(!host.mounts.is_empty());
    app.panel_content_action(&id("editor"), &ControlId::new("apply").unwrap()).unwrap();
    assert_eq!(probe.state(&key).unwrap().applies, 1);
    app.run_panel_command("layout.save", &serde_json::json!({"name":"Forwarding retained owners"})).unwrap();
    app.run_panel_command("layout.workspace.select", &serde_json::json!({"workspace":"fixture.other"})).unwrap();
    app.run_panel_command("layout.apply", &serde_json::json!({"name":"Forwarding retained owners"})).unwrap();
    app.tick_panel_runtime().unwrap();
    assert_eq!(app.panel_runtime().unwrap().controller_count(), 3);
    prepare_app_commands(&mut app);
    app.dispatch_panel_layout(LayoutAction::Replace { expected_revision: None, layout: Layout::empty(WorkspaceId::new("fixture.other").unwrap()) })
        .unwrap();
    let updates = probe.state(&key).unwrap().updates;
    app.tick_panel_runtime().unwrap();
    assert!(probe.state(&key).unwrap().updates > updates);
    prepare_app_commands(&mut app);
    app.run_panel_command("layout.workspace.select", &serde_json::json!({"workspace":"fixture.workspace"})).unwrap();
    app.tick_panel_runtime().unwrap();
    assert_eq!(probe.state(&key).unwrap().draft, "draft through app");
    assert!(probe.state(&key).unwrap().transaction_live);
    assert_eq!(probe.state(&key).unwrap().cancels, 0);
    assert_eq!(app.panel_runtime().unwrap().layout().layout(), &saved);
    let trace = trace.borrow();
    assert_eq!(trace.content_calls, 1);
    assert!(trace.service_borrows >= 10);
    assert!(trace.refreshes >= 4);
    assert_eq!(app.session.revision, revision);
    assert_eq!(app.session.undo.len(), undo);
    assert_eq!(app.session.is_dirty(), dirty);
    assert!(std::sync::Arc::ptr_eq(&doc, &app.session.doc));
    assert!(std::sync::Arc::ptr_eq(&model, &app.session.model.state()));
}

#[derive(Clone, Default)]
struct CurrentContextModels(std::rc::Rc<std::cell::RefCell<std::collections::BTreeMap<DocumentId, bool>>>);
impl PanelModels for CurrentContextModels {
    fn read(&self, owner: &ContentOwner, key: &str) -> PanelResult<serde_json::Value> {
        let ContentOwner::Document(document) = owner else {
            return Err(error(Rule::Availability, "fixture needs document owner"));
        };
        if key != "fixture.editable" {
            return Err(error(Rule::MissingTarget, "unknown fixture read"));
        }
        Ok(serde_json::json!(self.0.borrow().get(document).copied().unwrap_or(false)))
    }
}
struct CurrentContextServices {
    models: CurrentContextModels,
    commands: FakeContentCommands,
    local: FakeLocalServices,
    refreshes: std::rc::Rc<std::cell::RefCell<Vec<(String, String)>>>,
}
impl AppPanelServices for CurrentContextServices {
    fn refresh_read_only(&mut self, session: &solidcraft_engine::Session) -> PanelResult<()> {
        let editable = session.doc.param("fixture_editable").is_some_and(|param| param.expr.trim() == "1");
        self.models.0.borrow_mut().insert(DocumentId::new(session.document_id.clone())?, editable);
        self.refreshes.borrow_mut().push((session.document_id.clone(), session.doc.name.clone()));
        Ok(())
    }
    fn services(&mut self) -> ContentServices<'_> {
        ContentServices { models: &self.models, commands: &mut self.commands, local: &mut self.local }
    }
}
fn current_context_app() -> (crate::SolveApp, FixtureProbe) {
    let PanelMountHarness { runtime, probe, .. } = PanelMountHarness::new(false).unwrap();
    let mut session = solidcraft_engine::Session::default();
    session.execute("file.new", &serde_json::json!({"name":"Same name"})).unwrap();
    session.document_id = "context-a".into();
    session.execute("parameters.change", &serde_json::json!({"name":"fixture_editable","expression":1,"unit":""})).unwrap();
    let mut app = crate::SolveApp::new(session, Default::default());
    app.enable_panel_runtime(
        runtime,
        Box::new(CurrentContextServices {
            models: CurrentContextModels::default(),
            commands: Default::default(),
            local: Default::default(),
            refreshes: Default::default(),
        }),
    )
    .unwrap();
    prepare_app_commands(&mut app);
    (app, probe)
}
fn context_control(app: &mut crate::SolveApp, method: &str, params: serde_json::Value) -> serde_json::Value {
    let (request, _) = crate::control::ControlRequest::new(method, params);
    match crate::control::handle(app, &egui::Context::default(), &request) {
        crate::control::Outcome::Done(value) => value,
        _ => panic!("unexpected screenshot"),
    }
}

#[test]
fn application_command_query_and_failure_refresh_same_named_document_before_any_tick() {
    let (mut app, probe) = current_context_app();
    prepare_app_commands(&mut app);
    app.run("panel.open", panel_command_editor()).unwrap();
    let a = ControllerKey { instance: id("editor"), owner: ContentOwner::Document(DocumentId::new("context-a").unwrap()) };
    probe.edit(&a, |state| state.draft = "document A draft".into()).unwrap();
    let mut b = solidcraft_engine::Session::default();
    b.execute("file.new", &serde_json::json!({"name":"Same name"})).unwrap();
    b.document_id = "context-b".into();
    app.session.replace_design(b);
    let document = app.session.doc.clone();
    let model = app.session.model.state();
    let camera = app.cam;
    let before = (app.session.revision, app.session.undo.len(), app.session.is_dirty());
    let query = app.run("panel.list", serde_json::json!({})).unwrap();
    assert_eq!(query["state"]["availability"]["editor"]["state"], "unavailable");
    let failure = app.run("panel.focus", serde_json::json!({"target":panel_command_editor_target()})).unwrap_err();
    assert_eq!(commands::structured_error(&failure).unwrap()["rule"], "availability");
    assert_eq!(app.panel_runtime().unwrap().environment().document, Some(DocumentId::new("context-b").unwrap()));
    assert_eq!(probe.state(&a).unwrap().draft, "document A draft");
    assert_eq!(probe.state(&a).unwrap().cancels, 0);
    assert_eq!((app.session.revision, app.session.undo.len(), app.session.is_dirty()), before);
    assert!(std::sync::Arc::ptr_eq(&document, &app.session.doc));
    assert!(std::sync::Arc::ptr_eq(&model, &app.session.model.state()));
    assert_eq!(app.cam, camera);
}

#[test]
fn application_atomic_boundary_refreshes_same_document_context_for_query_and_mutation() {
    let (mut app, _) = current_context_app();
    prepare_app_commands(&mut app);
    app.run("panel.open", panel_command_editor()).unwrap();
    app.run("parameters.change", serde_json::json!({"name":"fixture_editable","expression":0})).unwrap();
    let query = app.run_panel_batch(&[serde_json::json!({"command":"panel.list","params":null})]).unwrap();
    assert_eq!(query["results"][0]["state"]["availability"]["editor"]["state"], "unavailable");
    let revision = app.session.revision;
    let model = app.session.model.state();
    assert_eq!(
        app.run_panel_batch(&[serde_json::json!({"command":"panel.focus","params":{"target":panel_command_editor_target()}})]).unwrap_err().rule,
        Rule::Availability
    );
    assert_eq!(app.session.revision, revision);
    assert!(std::sync::Arc::ptr_eq(&model, &app.session.model.state()));
    app.run("parameters.change", serde_json::json!({"name":"fixture_editable","expression":1})).unwrap();
    let result = app
        .run_panel_batch(&[
            serde_json::json!({"command":"panel.focus","params":{"target":panel_command_editor_target()}}),
            serde_json::json!({"command":"panel.list"}),
        ])
        .unwrap();
    assert_eq!(result["results"][1]["state"]["availability"]["editor"]["state"], "available");
}

#[test]
fn unsupported_atomic_intent_mixtures_refuse_before_context_or_controller_publication() {
    let (mut app, probe) = current_context_app();
    prepare_app_commands(&mut app);
    app.run("panel.open", panel_command_editor()).unwrap();
    let key = ControllerKey { instance: id("editor"), owner: ContentOwner::Document(DocumentId::new("context-a").unwrap()) };
    let before = app.panel_runtime().unwrap().layout().inspect();
    let controllers = app.panel_runtime().unwrap().controller_count();
    let retained = probe.state(&key).unwrap();
    let mut next = solidcraft_engine::Session::default();
    next.document_id = "context-b".into();
    app.session.replace_design(next);
    for commands in [
        serde_json::json!([{ "command":"panel.close","params":{"target":panel_command_editor_target()} },{"command":"layout.reset"}]),
        serde_json::json!([{"command":"layout.reset"},{ "command":"panel.close","params":{"target":panel_command_editor_target()} }]),
    ] {
        assert_eq!(app.run_panel_batch(commands.as_array().unwrap()).unwrap_err().rule, Rule::Capability);
        assert_eq!(app.panel_runtime().unwrap().layout().inspect(), before);
        assert_eq!(app.panel_runtime().unwrap().controller_count(), controllers);
        assert_eq!(app.panel_runtime().unwrap().environment().document, Some(DocumentId::new("context-a").unwrap()));
        assert_eq!(probe.state(&key).unwrap().last_state, retained.last_state);
        assert_eq!(probe.state(&key).unwrap().cancels, retained.cancels);
    }
}

#[test]
fn sequential_application_routes_refresh_after_file_new_and_same_document_model_changes() {
    for method in ["engine.script", "engine.batch"] {
        let (mut app, probe) = current_context_app();
        prepare_app_commands(&mut app);
        app.run("panel.open", panel_command_editor()).unwrap();
        let a = ControllerKey { instance: id("editor"), owner: ContentOwner::Document(DocumentId::new("context-a").unwrap()) };
        probe.edit(&a, |state| state.draft = "same instance, outgoing owner".into()).unwrap();
        let reply = context_control(
            &mut app,
            method,
            serde_json::json!({"rollback":false,"commands":[
                {"command":"file.new","params":{"name":"Same name"}},
                {"command":"panel.open","params":{"kind":"fixture.editor","instance":"editor"}},
            ]}),
        );
        let reason = if method == "engine.batch" { &reply["result"]["failed"]["panel_error"] } else { &reply["panel_error"] };
        assert_eq!(reason["rule"], "availability", "{reply}");
        assert_ne!(app.session.document_id, "context-a");
        assert_eq!(app.session.doc.name, "Same name");
        assert_eq!(app.panel_runtime().unwrap().environment().document.as_ref().unwrap().0, app.session.document_id);
        assert_eq!(probe.state(&a).unwrap().draft, "same instance, outgoing owner");
        assert_eq!(probe.state(&a).unwrap().cancels, 0);
        let success = context_control(
            &mut app,
            method,
            serde_json::json!({"rollback":false,"commands":[
                {"command":"parameters.change","params":{"name":"fixture_editable","expression":1,"unit":""}},
                {"command":"panel.focus","params":{"target":panel_command_editor_target()}},
            ]}),
        );
        assert_eq!(success["ok"], true, "{success}");
        assert!(success["result"].get("failed").is_none(), "{success}");
        let failure = context_control(
            &mut app,
            method,
            serde_json::json!({"rollback":false,"commands":[
                {"command":"parameters.change","params":{"name":"fixture_editable","expression":0}},
                {"command":"panel.focus","params":{"target":panel_command_editor_target()}},
            ]}),
        );
        let reason = if method == "engine.batch" { &failure["result"]["failed"]["panel_error"] } else { &failure["panel_error"] };
        assert_eq!(reason["rule"], "availability", "{failure}");
    }
}

#[derive(Clone)]
struct SharedCommandNative(std::rc::Rc<std::cell::RefCell<FakeNativeAdapter>>);
impl NativeHostAdapter for SharedCommandNative {
    fn capabilities(&self) -> HostCapabilities {
        self.0.borrow().capabilities()
    }
    fn prepare(&self, effects: &[HostEffect]) -> PanelResult<()> {
        self.0.borrow().prepare(effects)
    }
    fn commit(&mut self, effects: &[HostEffect]) {
        self.0.borrow_mut().commit(effects);
    }
}

#[test]
fn solve_app_gui_and_command_routes_share_native_owner_and_refuse_adapter_hot_swap() {
    let PanelMountHarness { runtime, probe, .. } = PanelMountHarness::new(true).unwrap();
    let catalogue = fixture_command_catalogue(runtime.layout().registry()).unwrap();
    let native = std::rc::Rc::new(std::cell::RefCell::new(FakeNativeAdapter::new(true)));
    let mut app = crate::SolveApp::new(solidcraft_engine::Session::default(), Default::default());
    app.enable_panel_runtime(runtime, Box::new(FakeAppPanelServices { models: FakeModels { editable: true }, ..Default::default() })).unwrap();
    app.panel_runtime_mut().unwrap().set_command_adapters(Box::new(catalogue), Box::new(SharedCommandNative(native.clone()))).unwrap();
    prepare_app_commands(&mut app);
    app.run("panel.open", panel_command_editor()).unwrap();
    let key = ControllerKey { instance: id("editor"), owner: ContentOwner::Document(DocumentId::new(app.session.document_id.clone()).unwrap()) };
    probe
        .edit(&key, |state| {
            state.draft = "native draft".into();
            state.reject_cancel = true;
        })
        .unwrap();
    let destination: Destination = serde_json::from_value(panel_command_native_move()["destination"].clone()).unwrap();
    prepare_app_commands(&mut app);
    app.dispatch_panel_layout(transaction(vec![Mutation::Move { instance: id("editor"), destination }])).unwrap();
    assert_eq!(native.borrow().hosts.len(), 1);
    assert!(
        app.panel_runtime_mut()
            .unwrap()
            .set_command_adapters(Box::<commands::InMemoryCatalogue>::default(), Box::new(FakeNativeAdapter::new(true)))
            .is_err()
    );
    assert!(app.disable_panel_runtime().is_err());
    assert_eq!(native.borrow().hosts.len(), 1);
    app.run("panel.native_close", serde_json::json!({"target":{"type":"native_host","id":"native-host"}})).unwrap();
    assert!(native.borrow().hosts.is_empty(), "command redocks the GUI-created host through its actual owner");
    app.run("panel.popout", panel_command_native_move()).unwrap();
    native.borrow_mut().fail_prepare = true;
    let before = app.panel_runtime().unwrap().layout().inspect();
    let hosts = native.borrow().hosts.clone();
    prepare_app_commands(&mut app);
    assert_eq!(
        app.dispatch_panel_layout(transaction(vec![Mutation::NativeClose { host: HostId::new("native-host").unwrap() }])).unwrap_err().rule,
        Rule::Capability
    );
    assert_eq!(app.panel_runtime().unwrap().layout().inspect(), before);
    assert_eq!(native.borrow().hosts, hosts);
    native.borrow_mut().fail_prepare = false;
    prepare_app_commands(&mut app);
    app.dispatch_panel_layout(transaction(vec![Mutation::NativeClose { host: HostId::new("native-host").unwrap() }])).unwrap();
    assert!(native.borrow().hosts.is_empty(), "GUI redocks the command-created host through the same owner");
    assert_eq!(probe.state(&key).unwrap().draft, "native draft");
    assert_eq!(probe.state(&key).unwrap().cancels, 0);
}

#[derive(Default)]
struct BoundaryObservation {
    document: Option<DocumentId>,
    editable: std::collections::BTreeMap<DocumentId, bool>,
    refreshes: usize,
    fail_refresh: bool,
    fail_factory: bool,
    commands: Vec<ContentOwner>,
    keyboard: std::collections::BTreeMap<LocalScope, InstanceId>,
    releases: Vec<(LocalScope, InstanceId)>,
    factory_keys: Vec<ControllerKey>,
    model_reads: Vec<ContentOwner>,
}
type BoundaryProbe = std::rc::Rc<std::cell::RefCell<BoundaryObservation>>;
struct BoundaryModels(BoundaryProbe);
impl PanelModels for BoundaryModels {
    fn read(&self, owner: &ContentOwner, key: &str) -> PanelResult<serde_json::Value> {
        if key != "fixture.editable" {
            return Err(error(Rule::MissingTarget, "unknown boundary model key"));
        }
        let mut state = self.0.borrow_mut();
        state.model_reads.push(owner.clone());
        let available = match owner {
            ContentOwner::Document(document) => state.editable.get(document).copied().unwrap_or(false),
            _ => true,
        };
        Ok(serde_json::json!(available))
    }
}
struct BoundaryCommands(BoundaryProbe);
impl ContentCommandSink for BoundaryCommands {
    fn execute(&mut self, owner: &ContentOwner, _command: &str, _input: serde_json::Value) -> PanelResult<serde_json::Value> {
        let mut state = self.0.borrow_mut();
        if matches!(owner, ContentOwner::Document(document) if Some(document) != state.document.as_ref()) {
            return Err(error(Rule::Placement, "content command addressed the outgoing document"));
        }
        state.commands.push(owner.clone());
        Ok(serde_json::Value::Null)
    }
}
struct BoundaryLocal(BoundaryProbe);
impl PanelLocalServices for BoundaryLocal {
    fn request(&mut self, scope: &LocalScope, request: LocalRequest) -> PanelResult<()> {
        scope.validate()?;
        request.validate()?;
        let mut state = self.0.borrow_mut();
        if scope.document != state.document {
            return Err(error(Rule::Placement, "local request addressed the outgoing document"));
        }
        if let LocalRequest::KeyboardOwner { instance, .. } = request {
            state.keyboard.insert(scope.clone(), instance);
        }
        Ok(())
    }
    fn release(&mut self, scope: &LocalScope, instance: &InstanceId) {
        let mut state = self.0.borrow_mut();
        if state.keyboard.get(scope) == Some(instance) {
            state.keyboard.remove(scope);
        }
        state.releases.push((scope.clone(), instance.clone()));
    }
}
struct BoundaryServices {
    trace: BoundaryProbe,
    models: BoundaryModels,
    commands: BoundaryCommands,
    local: BoundaryLocal,
}
impl BoundaryServices {
    fn new(trace: BoundaryProbe) -> Self {
        Self { models: BoundaryModels(trace.clone()), commands: BoundaryCommands(trace.clone()), local: BoundaryLocal(trace.clone()), trace }
    }
}
impl AppPanelServices for BoundaryServices {
    fn refresh_read_only(&mut self, session: &solidcraft_engine::Session) -> PanelResult<()> {
        let mut state = self.trace.borrow_mut();
        state.refreshes += 1;
        if state.fail_refresh {
            return Err(error(Rule::Availability, "boundary refresh refused"));
        }
        let document = DocumentId::new(session.document_id.clone())?;
        state.editable.entry(document.clone()).or_insert(true);
        state.document = Some(document);
        Ok(())
    }
    fn services(&mut self) -> ContentServices<'_> {
        ContentServices { models: &self.models, commands: &mut self.commands, local: &mut self.local }
    }
}

fn boundary_app() -> (crate::scenario::Harness, FixtureProbe, BoundaryProbe) {
    let mut h = crate::scenario::Harness::new();
    h.app.home.open = false;
    let PanelMountHarness { mut runtime, probe, .. } = PanelMountHarness::without_content(true).unwrap();
    let trace = BoundaryProbe::default();
    for (kind, editor, ownership) in
        [("fixture.editor", true, Ownership::Document), ("fixture.log", false, Ownership::Workspace), ("fixture.pin", false, Ownership::Workspace)]
    {
        let binding = fixture_controller_binding(&probe, editor, ownership);
        let create = binding.create;
        let failure = trace.clone();
        runtime
            .register(
                PanelKind::new(kind).unwrap(),
                ControllerBinding {
                    ownership,
                    create: Box::new(move |instance, owner| {
                        failure.borrow_mut().factory_keys.push(ControllerKey { instance: instance.id.clone(), owner: owner.clone() });
                        if editor && failure.borrow().fail_factory {
                            return Err(error(Rule::Lifecycle, "incoming factory refused"));
                        }
                        create(instance, owner)
                    }),
                },
            )
            .unwrap();
    }
    h.app.disable_panel_runtime().unwrap();
    h.app.enable_panel_runtime(runtime, Box::new(BoundaryServices::new(trace.clone()))).unwrap();
    prepare_app_commands(&mut h.app);
    h.app.dispatch_panel_layout(transaction(vec![fixture_open("fixture.editor", "editor", "editor-section", Zone::Left, 0).unwrap()])).unwrap();
    h.frames(3);
    (h, probe, trace)
}

fn app_editor_key(app: &crate::SolveApp) -> ControllerKey {
    ControllerKey { instance: id("editor"), owner: ContentOwner::Document(DocumentId::new(app.session.document_id.clone()).unwrap()) }
}

/// Exercises the real app ordering: logic has ALREADY ticked, then a control/
/// shortcut changes Session, then app UI (including app-bar tabs) mounts the host.
fn boundary_frame(h: &mut crate::scenario::Harness, events: Vec<egui::Event>, mut after_logic: impl FnMut(&mut crate::SolveApp)) -> egui::FullOutput {
    let time = h.ctx.input(|input| input.time) + 1.0 / 60.0;
    let mut output = h.ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0))),
            events,
            time: Some(time),
            focused: true,
            ..Default::default()
        },
        |ui| {
            h.app.logic(ui.ctx());
            if ui.ctx().current_pass_index() == 0 {
                after_logic(&mut h.app);
            }
            h.app.ui(ui);
        },
    );
    output.textures_delta.clear();
    output
}

fn app_control(app: &crate::SolveApp, target: HostControlTarget) -> HostFrameControl {
    app.universal_panel_frame().unwrap().controls.iter().find(|control| control.target == target).unwrap().clone()
}
fn button_event(at: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE }
}

#[test]
fn same_frame_session_switch_after_logic_cancels_old_capture_menu_focus_and_routes_incoming_actions() {
    for captured in [false, true] {
        let (mut h, probe, trace) = boundary_app();
        let old_key = app_editor_key(&h.app);
        probe.edit(&old_key, |state| state.draft = "outgoing draft survives same-frame switch".into()).unwrap();
        let header = app_control(&h.app, HostControlTarget::Pointer(HostPointerTarget::Header { instance: id("editor") }));
        let at = egui_rect(header.bounds).center();
        h.ctx.memory_mut(|memory| memory.request_focus(header.id));
        h.frames(1);
        let old_scope = runtime_scope(h.app.panel_runtime().unwrap());
        h.app
            .panel_runtime_mut()
            .unwrap()
            .with_services(|_, services| {
                services.local.request(&old_scope, LocalRequest::KeyboardOwner { instance: id("editor"), control: ControlId::new("draft").unwrap() })
            })
            .unwrap();
        if captured {
            h.frame_with(vec![egui::Event::PointerMoved(at), button_event(at, true)]);
            h.frame_with(vec![egui::Event::PointerMoved(at + egui::vec2(8.0, 0.0))]);
            assert!(h.app.panel_runtime_mut().unwrap().host().unwrap().interaction().is_captured());
        } else {
            let menu = app_control(&h.app, HostControlTarget::Pointer(HostPointerTarget::Menu { instance: id("editor") }));
            let at = egui_rect(menu.bounds).center();
            h.frame_with(vec![egui::Event::PointerMoved(at), button_event(at, true)]);
            h.frame_with(vec![button_event(at, false)]);
            h.frames(2);
            assert!(h.app.universal_panel_frame().unwrap().menu_bounds.is_some());
        }
        let before = probe.state(&old_key).unwrap();
        let old_focus = h.ctx.memory(|memory| memory.focused());
        let saved = h.app.panel_runtime().unwrap().layout().layout().clone();
        let drop = egui::pos2(1500.0, 600.0);
        let incoming_session = solidcraft_engine::Session::default();
        let document = incoming_session.doc.clone();
        let model = incoming_session.model.state();
        let undo = incoming_session.undo.len();
        let dirty = incoming_session.is_dirty();
        let mut incoming = Some(incoming_session);
        let mut switched_revision = None;
        let output = boundary_frame(&mut h, vec![egui::Event::PointerMoved(drop), button_event(drop, false)], |app| {
            crate::documents::open_tab(app, incoming.take().unwrap()).unwrap();
            app.home.open = false;
            switched_revision = Some(app.session.revision);
        });
        let revision = switched_revision.unwrap();
        assert!(!output.shapes.is_empty(), "the real app UI path was traversed headlessly");
        let incoming_key = app_editor_key(&h.app);
        assert_ne!(incoming_key.owner, old_key.owner);
        assert_eq!(h.app.panel_runtime().unwrap().environment().document, Some(DocumentId::new(h.app.session.document_id.clone()).unwrap()));
        let frame = h.app.universal_panel_frame().unwrap();
        assert!(frame.preview.is_none() && frame.menu_bounds.is_none());
        assert!(!h.app.panel_runtime_mut().unwrap().host().unwrap().interaction().is_captured());
        assert_eq!(h.app.panel_runtime().unwrap().layout().layout(), &saved, "old release cannot move incoming content");
        assert_ne!(app_control(&h.app, header.target).id, header.id);
        assert_ne!(h.ctx.memory(|memory| memory.focused()), Some(header.id));
        if let Some(old_focus) = old_focus {
            assert_ne!(h.ctx.memory(|memory| memory.focused()), Some(old_focus));
        }
        assert!(!trace.borrow().keyboard.contains_key(&old_scope));
        let old = probe.state(&old_key).unwrap();
        assert_eq!(old.updates, before.updates + 1, "only the one logic tick updates outgoing services");
        assert_eq!(old.renders, before.renders, "outgoing owner cannot render after the Session switch");
        assert_eq!(old.draft, before.draft);
        assert_eq!((old.cancels, old.applies, old.destroys), (0, 0, 0));
        let incoming = probe.state(&incoming_key).unwrap();
        assert_eq!(incoming.updates, 0, "boundary reconciliation must not add a background tick");
        assert!(incoming.renders > 0, "incoming retained owner renders in the SAME frame");
        let updates = old.updates;
        h.app.panel_content_action(&id("editor"), &ControlId::new("apply").unwrap()).unwrap();
        h.app.dispatch_panel_layout(transaction(vec![Mutation::Focus { instance: id("editor") }])).unwrap();
        assert_eq!(probe.state(&incoming_key).unwrap().applies, 1);
        assert_eq!(probe.state(&old_key).unwrap().applies, 0);
        assert_eq!(probe.state(&old_key).unwrap().updates, updates);
        assert_eq!(probe.state(&incoming_key).unwrap().updates, 0);
        assert_eq!(trace.borrow().commands, vec![incoming_key.owner]);
        assert_eq!(h.app.session.revision, revision);
        assert_eq!(h.app.session.undo.len(), undo);
        assert_eq!(h.app.session.is_dirty(), dirty);
        assert!(std::sync::Arc::ptr_eq(&document, &h.app.session.doc));
        assert!(std::sync::Arc::ptr_eq(&model, &h.app.session.model.state()));
    }
}

#[test]
fn same_frame_refresh_refusal_fails_closed_before_render_dispatch_or_content_action() {
    for factory_refusal in [false, true] {
        let (mut h, probe, trace) = boundary_app();
        let key = app_editor_key(&h.app);
        probe.edit(&key, |state| state.draft = "retained through refresh refusal".into()).unwrap();
        let header = app_control(&h.app, HostControlTarget::Pointer(HostPointerTarget::Header { instance: id("editor") }));
        let at = egui_rect(header.bounds).center();
        h.frame_with(vec![egui::Event::PointerMoved(at), button_event(at, true)]);
        let before = probe.state(&key).unwrap();
        let saved = h.app.panel_runtime().unwrap().layout().inspect();
        let mut incoming = Some(solidcraft_engine::Session::default());
        boundary_frame(&mut h, vec![button_event(at + egui::vec2(500.0, 0.0), false)], |app| {
            crate::documents::open_tab(app, incoming.take().unwrap()).unwrap();
            app.home.open = false;
            if factory_refusal {
                trace.borrow_mut().fail_factory = true;
            } else {
                trace.borrow_mut().fail_refresh = true;
            }
        });
        assert!(h.app.universal_panel_frame().is_none());
        assert!(!h.app.panel_runtime_mut().unwrap().host().unwrap().interaction().is_captured());
        let expected = if factory_refusal { Rule::Lifecycle } else { Rule::Availability };
        assert_eq!(h.app.panel_content_action(&id("editor"), &ControlId::new("apply").unwrap()).unwrap_err().rule, expected);
        assert_eq!(h.app.dispatch_panel_layout(transaction(vec![Mutation::Close { instance: id("editor") }])).unwrap_err().rule, expected);
        assert_eq!(h.app.panel_runtime().unwrap().layout().inspect(), saved);
        assert_eq!(h.app.panel_runtime().unwrap().controller_count(), 1, "a refused incoming factory cannot publish another retained owner");
        let after = probe.state(&key).unwrap();
        assert_eq!(after.updates, before.updates + 1);
        assert_eq!(after.renders, before.renders);
        assert_eq!(after.draft, before.draft);
        assert_eq!((after.applies, after.cancels, after.destroys), (0, 0, 0));
        assert!(!h.app.panel_runtime().unwrap().state(&key).unwrap().presented);
        assert!(trace.borrow().commands.is_empty());
        trace.borrow_mut().fail_refresh = false;
        trace.borrow_mut().fail_factory = false;
        h.frames(1);
        assert_eq!(probe.state(&key).unwrap().draft, before.draft);
        assert!(h.app.panel_runtime().unwrap().state(&app_editor_key(&h.app)).unwrap().presented);
    }
}

#[test]
fn same_frame_programmatic_actions_and_mount_reconcile_a_switch_after_host_preparation() {
    let (mut h, probe, trace) = boundary_app();
    let old_key = app_editor_key(&h.app);
    let before = probe.state(&old_key).unwrap();
    let mut incoming = Some(solidcraft_engine::Session::default());
    let time = h.ctx.input(|input| input.time) + 1.0 / 60.0;
    let mut output =
        h.ctx.run_ui(egui::RawInput { screen_rect: Some(egui_rect(capacity())), time: Some(time), focused: true, ..Default::default() }, |ui| {
            h.app.logic(ui.ctx());
            if ui.ctx().current_pass_index() == 0 {
                h.app.panel_runtime.as_mut().unwrap().prepare_host(&h.app.session, capacity()).unwrap();
                crate::documents::open_tab(&mut h.app, incoming.take().unwrap()).unwrap();
                h.app.home.open = false;
                // Neither call may borrow outgoing cached models/owner services, and
                // neither is allowed to add another background tick.
                h.app.panel_content_action(&id("editor"), &ControlId::new("apply").unwrap()).unwrap();
                h.app.dispatch_panel_layout(transaction(vec![Mutation::Focus { instance: id("editor") }])).unwrap();
            }
            h.app.panel_runtime.as_mut().unwrap().show_host(&h.app.session, ui).unwrap();
        });
    output.textures_delta.clear();
    let incoming_key = app_editor_key(&h.app);
    assert_ne!(old_key.owner, incoming_key.owner);
    assert_eq!(probe.state(&old_key).unwrap().updates, before.updates + 1);
    assert_eq!(probe.state(&old_key).unwrap().renders, before.renders);
    assert_eq!(probe.state(&old_key).unwrap().applies, 0);
    assert_eq!(probe.state(&incoming_key).unwrap().updates, 0);
    assert_eq!(probe.state(&incoming_key).unwrap().applies, 1);
    assert!(probe.state(&incoming_key).unwrap().renders > 0);
    assert_eq!(trace.borrow().commands, vec![incoming_key.owner]);
    assert_eq!(h.app.panel_runtime().unwrap().environment().document, Some(DocumentId::new(h.app.session.document_id.clone()).unwrap()));
    let incoming_header = app_control(&h.app, HostControlTarget::Pointer(HostPointerTarget::Header { instance: id("editor") }));
    assert_eq!(
        h.ctx.memory(|memory| memory.focused()),
        Some(incoming_header.id),
        "explicit incoming Focus targets the current owner's actual widget"
    );
}

#[test]
fn same_frame_current_document_availability_is_reconciled_without_another_service_tick() {
    let (mut h, probe, trace) = boundary_app();
    let key = app_editor_key(&h.app);
    let document = DocumentId::new(h.app.session.document_id.clone()).unwrap();
    let before = probe.state(&key).unwrap();
    boundary_frame(&mut h, Vec::new(), |_| {
        trace.borrow_mut().editable.insert(document.clone(), false);
    });
    assert_eq!(probe.state(&key).unwrap().updates, before.updates + 1);
    assert_eq!(probe.state(&key).unwrap().renders, before.renders);
    assert!(!h.app.universal_panel_frame().unwrap().layout.resolved.panels.iter().any(|panel| panel.instance == id("editor")));
    assert_eq!(h.app.panel_content_action(&id("editor"), &ControlId::new("apply").unwrap()).unwrap_err().rule, Rule::Availability);
    assert_eq!(probe.state(&key).unwrap().updates, before.updates + 1);
    assert!(trace.borrow().commands.is_empty());
}

#[derive(Clone)]
struct SharedNative(std::rc::Rc<std::cell::RefCell<FakeNativeAdapter>>);
impl NativeHostAdapter for SharedNative {
    fn capabilities(&self) -> HostCapabilities {
        self.0.borrow().capabilities.clone()
    }
    fn prepare(&self, effects: &[HostEffect]) -> PanelResult<()> {
        self.0.borrow().prepare(effects)
    }
    fn commit(&mut self, effects: &[HostEffect]) {
        self.0.borrow_mut().commit(effects);
    }
}

fn native_placement(instance: &str, section_id: &str, host_id: &str, index: usize) -> Mutation {
    Mutation::Move {
        instance: id(instance),
        destination: Destination::NewSection {
            section: section(section_id),
            index,
            location: SectionLocation::Window {
                host: HostId::new(host_id).unwrap(),
                origin: Point { x: 0.0, y: 0.0 },
                return_to: DockReturn {
                    zone: Zone::Left,
                    index: 0,
                    size: Size { width: 280.0, height: 240.0 },
                    collapsed: false,
                    join: None,
                    section: None,
                    column: None,
                    column_index: 0,
                    column_width: None,
                    weight: 1.0,
                    preferred_extent: None,
                    extent_priority: 0,
                },
            },
            size: Size { width: 280.0, height: 240.0 },
        },
    }
}

#[test]
fn app_native_owner_is_persistent_with_host_off_or_on_and_refusals_preserve_resources() {
    for shared_host in [false, true] {
        let PanelMountHarness { runtime, probe, .. } = PanelMountHarness::new(true).unwrap();
        let trace = BoundaryProbe::default();
        let mut app = crate::SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        app.enable_panel_runtime(runtime, Box::new(BoundaryServices::new(trace.clone()))).unwrap();
        prepare_app_commands(&mut app);
        app.dispatch_panel_layout(transaction(vec![fixture_open("fixture.editor", "editor", "editor-section", Zone::Left, 0).unwrap()])).unwrap();

        let key = app_editor_key(&app);
        probe.edit(&key, |state| state.draft = "native owner retains this draft".into()).unwrap();
        let action = transaction(vec![native_placement("editor", "native-editor", "owned-native", 0)]);
        let before = app.panel_runtime().unwrap().layout().inspect();
        assert_eq!(app.dispatch_panel_layout(action.clone()).unwrap_err().rule, Rule::Capability, "default owner cannot create native resources");
        assert_eq!(app.panel_runtime().unwrap().layout().inspect(), before);
        let invalid = std::rc::Rc::new(std::cell::RefCell::new(FakeNativeAdapter::new(true)));
        invalid.borrow_mut().capabilities.max_native_hosts = MAX_SECTIONS + 1;
        assert_eq!(app.set_panel_native_adapter(Box::new(SharedNative(invalid.clone()))).unwrap_err().rule, Rule::Capability);
        assert!(!app.panel_runtime_mut().unwrap().native_capabilities().native_windows);
        assert!(invalid.borrow().hosts.is_empty());
        let native = std::rc::Rc::new(std::cell::RefCell::new(FakeNativeAdapter::new(true)));
        app.set_panel_native_adapter(Box::new(SharedNative(native.clone()))).unwrap();
        native.borrow_mut().fail_prepare = true;
        assert_eq!(app.dispatch_panel_layout(action.clone()).unwrap_err().rule, Rule::Capability);
        assert_eq!(app.panel_runtime().unwrap().layout().inspect(), before);
        assert!(native.borrow().hosts.is_empty());
        assert_eq!(native.borrow().commits, 0);
        native.borrow_mut().fail_prepare = false;
        prepare_app_commands(&mut app);
        app.dispatch_panel_layout(action).unwrap();
        assert_eq!(native.borrow().hosts.get(&HostId::new("owned-native").unwrap()), Some(&section("native-editor")));
        if !shared_host {
            app.disable_universal_panel_host().unwrap();
        }
        let after_create = app.panel_runtime().unwrap().layout().inspect();
        let commits = native.borrow().commits;
        let replacement = std::rc::Rc::new(std::cell::RefCell::new(FakeNativeAdapter::new(true)));
        assert_eq!(app.set_panel_native_adapter(Box::new(SharedNative(replacement.clone()))).unwrap_err().rule, Rule::Lifecycle);
        assert!(replacement.borrow().hosts.is_empty());
        assert_eq!(native.borrow().hosts.len(), 1);
        probe.edit(&key, |state| state.reject_cancel = true).unwrap();
        assert_eq!(app.disable_panel_runtime().unwrap_err().rule, Rule::Lifecycle);
        assert_eq!(app.panel_runtime().unwrap().layout().inspect(), after_create);
        assert_eq!(native.borrow().commits, commits);
        assert_eq!(native.borrow().hosts.len(), 1);
        assert_eq!(probe.state(&key).unwrap().destroys, 0);
        probe.edit(&key, |state| state.reject_cancel = false).unwrap();
        native.borrow_mut().fail_prepare = true;
        assert_eq!(app.disable_panel_runtime().unwrap_err().rule, Rule::Capability);
        assert_eq!(app.panel_runtime().unwrap().layout().inspect(), after_create);
        assert_eq!(native.borrow().commits, commits);
        assert_eq!(probe.state(&key).unwrap().destroys, 0);
        native.borrow_mut().fail_prepare = false;
        app.disable_panel_runtime().unwrap();
        assert!(app.panel_runtime().is_none());
        assert!(native.borrow().hosts.is_empty(), "the SAME installed owner releases its native resources");
        assert_eq!(native.borrow().commits, commits + 1);
        assert_eq!(probe.state(&key).unwrap().draft, "native owner retains this draft");
        assert_eq!(probe.state(&key).unwrap().destroys, 1);
    }
}

fn mount_app_root(app: &mut crate::SolveApp, ctx: &egui::Context) {
    let session = &app.session;
    let integration = app.panel_runtime.as_mut().unwrap();
    integration.prepare_host(session, capacity()).unwrap();
    let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(egui_rect(capacity())), focused: true, ..Default::default() }, |ui| {
        integration.show_host(session, ui).unwrap();
    });
    output.textures_delta.clear();
}

#[test]
fn app_root_and_native_cohorts_alternate_mounts_without_releasing_other_hosts() {
    let (mut h, probe, trace) = boundary_app();
    let native = std::rc::Rc::new(std::cell::RefCell::new(FakeNativeAdapter::new(true)));
    h.app.set_panel_native_adapter(Box::new(SharedNative(native.clone()))).unwrap();
    h.app
        .dispatch_panel_layout(transaction(vec![
            fixture_open("fixture.log", "native-a", "a", Zone::Right, 1).unwrap(),
            fixture_open("fixture.log", "native-b", "b", Zone::Right, 2).unwrap(),
        ]))
        .unwrap();
    h.app
        .dispatch_panel_layout(transaction(vec![
            native_placement("native-a", "native-section-a", "host-a", 1),
            native_placement("native-b", "native-section-b", "host-b", 2),
        ]))
        .unwrap();
    let all = resolve_geometry(h.app.panel_runtime().unwrap().layout(), capacity(), Direction::Ltr, 48.0).unwrap();
    let frame_for = |instance: &str| ResolvedLayout {
        panels: all.panels.iter().filter(|panel| panel.instance == id(instance)).cloned().collect(),
        ..Default::default()
    };
    let frame_a = frame_for("native-a");
    let frame_b = frame_for("native-b");
    assert_eq!((frame_a.panels.len(), frame_b.panels.len()), (1, 1));
    let host_a = HostId::new("host-a").unwrap();
    let host_b = HostId::new("host-b").unwrap();
    let mut renderer_a = FakePanelHost::default();
    let mut renderer_b = FakePanelHost::default();
    mount_app_root(&mut h.app, &h.ctx);
    h.app.mount_native_panels(&host_a, &frame_a, &mut renderer_a).unwrap();
    h.app.mount_native_panels(&host_b, &frame_b, &mut renderer_b).unwrap();
    let root_key = app_editor_key(&h.app);
    let workspace = h.app.panel_runtime().unwrap().environment().workspace.clone();
    let a_key = ControllerKey { instance: id("native-a"), owner: ContentOwner::Workspace(workspace.clone()) };
    let b_key = ControllerKey { instance: id("native-b"), owner: ContentOwner::Workspace(workspace) };
    let root_scope = runtime_scope(h.app.panel_runtime().unwrap());
    let mut a_scope = root_scope.clone();
    a_scope.viewport = ViewportId::new("host-a").unwrap();
    let mut b_scope = root_scope.clone();
    b_scope.viewport = ViewportId::new("host-b").unwrap();
    for (scope, instance) in [(&root_scope, "editor"), (&a_scope, "native-a"), (&b_scope, "native-b")] {
        h.app
            .panel_runtime_mut()
            .unwrap()
            .with_services(|_, services| {
                services.local.request(scope, LocalRequest::KeyboardOwner { instance: id(instance), control: ControlId::new("draft").unwrap() })
            })
            .unwrap();
    }
    let keyboard = trace.borrow().keyboard.clone();
    let releases = trace.borrow().releases.len();
    for _ in 0..3 {
        h.app.mount_native_panels(&host_b, &frame_b, &mut renderer_b).unwrap();
        mount_app_root(&mut h.app, &h.ctx);
        h.app.mount_native_panels(&host_a, &frame_a, &mut renderer_a).unwrap();
        for key in [&root_key, &a_key, &b_key] {
            let state = h.app.panel_runtime().unwrap().state(key).unwrap();
            assert!(state.presented && state.activated, "partial viewport frames cannot deactivate {key:?}");
        }
        assert_eq!(trace.borrow().keyboard, keyboard);
        assert_eq!(trace.borrow().releases.len(), releases);
    }
    assert!(renderer_a.mounts.iter().all(|record| record.context.native_host.as_ref() == Some(&host_a) && record.context.viewport.0 == host_a.0));
    assert!(renderer_b.mounts.iter().all(|record| record.context.native_host.as_ref() == Some(&host_b) && record.context.viewport.0 == host_b.0));
    assert_eq!(h.app.mount_native_panels(&host_a, &frame_b, &mut renderer_a).unwrap_err().rule, Rule::Placement);
    assert_eq!(trace.borrow().keyboard, keyboard, "a foreign cohort rejects before lifecycle/local release");
    h.app.mount_native_panels(&host_a, &ResolvedLayout::default(), &mut renderer_a).unwrap();
    assert!(!h.app.panel_runtime().unwrap().state(&a_key).unwrap().presented);
    assert!(h.app.panel_runtime().unwrap().state(&root_key).unwrap().presented);
    assert!(h.app.panel_runtime().unwrap().state(&b_key).unwrap().presented);
    assert!(!trace.borrow().keyboard.contains_key(&a_scope));
    assert_eq!(trace.borrow().keyboard.get(&root_scope), Some(&id("editor")));
    assert_eq!(trace.borrow().keyboard.get(&b_scope), Some(&id("native-b")));
    h.app.disable_universal_panel_host().unwrap();
    assert!(!h.app.panel_runtime().unwrap().state(&root_key).unwrap().presented);
    assert!(h.app.panel_runtime().unwrap().state(&b_key).unwrap().presented);
    assert!(!trace.borrow().keyboard.contains_key(&root_scope));
    assert_eq!(trace.borrow().keyboard.get(&b_scope), Some(&id("native-b")));
    assert_eq!(
        h.app.mount_panels(&frame_b, &mut renderer_b).unwrap_err().rule,
        Rule::Placement,
        "generic root mounting cannot masquerade as a partial native frame even with the shared host off"
    );
    assert!(h.app.panel_runtime().unwrap().state(&b_key).unwrap().presented);
    assert_eq!(trace.borrow().keyboard.get(&b_scope), Some(&id("native-b")));
    assert_eq!(native.borrow().hosts.len(), 2, "unmounting presentation is not native resource teardown");
    assert_eq!(probe.state(&root_key).unwrap().cancels, 0);
    h.app.disable_panel_runtime().unwrap();
    assert!(native.borrow().hosts.is_empty());
    assert!(trace.borrow().keyboard.is_empty());
}

#[test]
fn foreign_foreground_widgets_block_raw_header_and_sash_presses_at_both_scales() {
    for scale in [1.0, 2.0] {
        for sash in [false, true] {
            for (same_frame_release, warm_cover) in [(false, false), (false, true), (true, false), (true, true)] {
                let mut content = PanelMountHarness::new(false).unwrap();
                open_editor(&mut content);
                content.dispatch(vec![fixture_open("fixture.log", "neighbour", "neighbour-section", Zone::Left, 1).unwrap()]).unwrap();
                let key = editor_key(&content);
                content.probe.edit(&key, |state| state.draft = "foreign window must not move this".into()).unwrap();
                let mut raw = RawPanelHost::new(content, scale);
                raw.prime_foreign_window();
                raw.frame(Vec::new(), false);
                let target = if sash {
                    let sash = raw.host.frame().unwrap().layout.sashes.first().unwrap();
                    HostControlTarget::Pointer(HostPointerTarget::Sash { first: sash.first.clone(), second: sash.second.clone() })
                } else {
                    HostControlTarget::Pointer(HostPointerTarget::Header { instance: id("editor") })
                };
                let at = raw.at(target.clone());
                let cover = egui::Rect::from_center_size(at, egui::vec2(180.0, 80.0));
                if warm_cover {
                    raw.foreign_cover = Some(cover);
                }
                raw.frame(Vec::new(), false);
                raw.frame(vec![egui::Event::PointerMoved(at)], false);
                let control = raw.host.frame().unwrap().controls.iter().find(|control| control.target == target).unwrap();
                let response = raw.ctx.read_response(control.id).unwrap();
                if warm_cover {
                    assert_ne!(raw.ctx.layer_id_at(at), Some(response.layer_id), "real unrelated Area covers the host control");
                }
                raw.foreign_cover = Some(cover);
                let before = raw.content.runtime.layout().inspect();
                let state = raw.content.probe.state(&key).unwrap();
                let native_commits = raw.content.native.commits;
                let drop = if sash { at + egui::vec2(0.0, 160.0) } else { egui::pos2(raw.capacity.size.width - 1.0, 550.0) };
                if same_frame_release {
                    raw.frame(
                        vec![egui::Event::PointerMoved(at), button_event(at, true), egui::Event::PointerMoved(drop), button_event(drop, false)],
                        true,
                    );
                } else {
                    raw.pointer(at, true, true);
                    if warm_cover {
                        assert!(
                            raw.foreign_responses.iter().any(|response| response.is_pointer_button_down_on()),
                            "actual foreign widget receives the press"
                        );
                    }
                    assert!(!raw.host.interaction().is_captured(), "covered chrome must not capture raw input");
                    raw.frame(vec![egui::Event::PointerMoved(drop)], false);
                    raw.pointer(drop, false, true);
                }
                assert_eq!(raw.content.runtime.layout().inspect(), before, "foreign input cannot mutate header/sash placement");
                assert!(!raw.host.interaction().is_captured());
                assert!(raw.host.frame().unwrap().preview.is_none());
                assert!(
                    !raw.host
                        .frame()
                        .unwrap()
                        .events
                        .iter()
                        .any(|event| matches!(event, HostEvent::Committed { .. } | HostEvent::PreparedCommitted { .. }))
                );
                assert_eq!(raw.content.native.commits, native_commits);
                let after = raw.content.probe.state(&key).unwrap();
                assert_eq!(after.draft, state.draft);
                assert_eq!(
                    (after.cancels, after.reparents, after.applies, after.destroys),
                    (state.cancels, state.reparents, state.applies, state.destroys)
                );
                assert_eq!(raw.content.commands.calls, 0);
            }
        }
    }
}

#[test]
fn overlapping_floating_sections_route_only_the_actual_top_widget_at_both_scales() {
    for scale in [1.0, 2.0] {
        for top_body_covers_header in [false, true] {
            let mut content = PanelMountHarness::new(false).unwrap();
            open_editor(&mut content);
            content
                .dispatch(vec![
                    Mutation::Move {
                        instance: id("editor"),
                        destination: Destination::NewSection {
                            section: section("editor-floating"),
                            index: 0,
                            location: SectionLocation::Floating { origin: Point { x: 200.0, y: 200.0 } },
                            size: Size { width: 320.0, height: 280.0 },
                        },
                    },
                    fixture_open("fixture.log", "top", "top-floating", Zone::Floating, 1).unwrap(),
                ])
                .unwrap();
            let top_origin = if top_body_covers_header { Point { x: 200.0, y: 100.0 } } else { Point { x: 200.0, y: 200.0 } };
            content
                .dispatch(vec![Mutation::Move {
                    instance: id("top"),
                    destination: Destination::NewSection {
                        section: section("top-floating"),
                        index: 1,
                        location: SectionLocation::Floating { origin: top_origin },
                        size: Size { width: 320.0, height: 360.0 },
                    },
                }])
                .unwrap();
            let key = editor_key(&content);
            content.probe.edit(&key, |state| state.draft = "bottom floating draft".into()).unwrap();
            let mut raw = RawPanelHost::new(content, scale);
            raw.frame(Vec::new(), false);
            raw.frame(Vec::new(), false);
            let lower = HostControlTarget::Pointer(HostPointerTarget::Header { instance: id("editor") });
            let at = raw.at(lower.clone());
            raw.frame(vec![egui::Event::PointerMoved(at)], false);
            let lower_id = raw.host.frame().unwrap().controls.iter().find(|control| control.target == lower).unwrap().id;
            assert_ne!(raw.ctx.layer_id_at(at), Some(raw.ctx.read_response(lower_id).unwrap().layer_id));
            let before = raw.content.runtime.layout().inspect();
            let native_commits = raw.content.native.commits;
            raw.pointer(at, true, true);
            assert!(!raw.ctx.read_response(lower_id).unwrap().is_pointer_button_down_on());
            if top_body_covers_header {
                assert!(!raw.host.interaction().is_captured());
            } else {
                assert!(raw.host.interaction().is_captured(), "top card's own header can capture");
            }
            let drop = egui::pos2(raw.capacity.size.width - 1.0, 550.0);
            raw.frame(vec![egui::Event::PointerMoved(drop)], false);
            raw.pointer(drop, false, true);
            assert_eq!(raw.content.runtime.layout().layout().containing(&id("editor")).unwrap().location.zone(), Zone::Floating);
            if top_body_covers_header {
                assert_eq!(raw.content.runtime.layout().inspect(), before);
                assert_eq!(raw.content.native.commits, native_commits);
            } else {
                assert_eq!(raw.content.runtime.layout().layout().containing(&id("top")).unwrap().location.zone(), Zone::Right);
                assert_eq!(raw.content.runtime.layout().inspect().revision, before.revision + 1);
            }
            assert_eq!(raw.content.probe.state(&key).unwrap().draft, "bottom floating draft");
            assert_eq!(raw.content.probe.state(&key).unwrap().reparents, 1, "only the initial explicit fixture move reparents the lower editor");
            assert_eq!(raw.content.commands.calls, 0);
        }
        // A floating body also covers a dock sash on a different real egui
        // layer. Resizing the obscured dock must not start underneath it.
        let mut content = PanelMountHarness::new(false).unwrap();
        open_editor(&mut content);
        content
            .dispatch(vec![
                fixture_open("fixture.log", "neighbour", "neighbour-section", Zone::Left, 1).unwrap(),
                fixture_open("fixture.log", "top", "top-floating", Zone::Floating, 2).unwrap(),
            ])
            .unwrap();
        let mut raw = RawPanelHost::new(content, scale);
        raw.frame(Vec::new(), false);
        let sash = raw.host.frame().unwrap().layout.sashes.first().unwrap().clone();
        let target = HostControlTarget::Pointer(HostPointerTarget::Sash { first: sash.first, second: sash.second });
        let at = raw.at(target.clone());
        raw.content
            .dispatch(vec![Mutation::Move {
                instance: id("top"),
                destination: Destination::NewSection {
                    section: section("top-floating"),
                    index: 2,
                    location: SectionLocation::Floating { origin: Point { x: 0.0, y: at.y - 100.0 } },
                    size: Size { width: 320.0, height: 360.0 },
                },
            }])
            .unwrap();
        raw.frame(Vec::new(), false);
        raw.frame(vec![egui::Event::PointerMoved(at)], false);
        let source = raw.host.frame().unwrap().controls.iter().find(|control| control.target == target).unwrap();
        assert_ne!(raw.ctx.layer_id_at(at), Some(raw.ctx.read_response(source.id).unwrap().layer_id));
        let before = raw.content.runtime.layout().inspect();
        raw.pointer(at, true, false);
        assert!(!raw.host.interaction().is_captured());
        raw.frame(vec![egui::Event::PointerMoved(at + egui::vec2(0.0, 100.0))], false);
        raw.pointer(at + egui::vec2(0.0, 100.0), false, true);
        assert_eq!(raw.content.runtime.layout().inspect(), before);
    }
}

#[test]
fn genuine_host_capture_survives_out_of_bounds_motion_and_a_foreign_release_layer() {
    for scale in [1.0, 2.0] {
        let mut content = PanelMountHarness::new(false).unwrap();
        open_editor(&mut content);
        let mut raw = RawPanelHost::new(content, scale);
        raw.prime_foreign_window();
        raw.frame(Vec::new(), false);
        raw.frame(Vec::new(), false);
        let at = raw.at(HostControlTarget::Pointer(HostPointerTarget::Header { instance: id("editor") }));
        raw.frame(vec![egui::Event::PointerMoved(at)], false);
        let before = raw.content.runtime.layout().inspect();
        raw.pointer(at, true, false);
        assert!(raw.host.interaction().is_captured());
        raw.frame(vec![egui::Event::PointerMoved(egui::pos2(-40.0, raw.capacity.size.height + 50.0))], false);
        assert!(raw.host.interaction().is_captured(), "out-of-bounds movement retains a genuine capture");
        let drop = egui::pos2(raw.capacity.size.width - 1.0, 550.0);
        raw.foreign_cover = Some(egui::Rect::from_center_size(drop, egui::vec2(80.0, 100.0)));
        raw.frame(vec![egui::Event::PointerMoved(drop)], false);
        raw.frame(Vec::new(), false);
        assert!(raw.host.interaction().is_captured());
        raw.pointer(drop, false, true);
        assert_eq!(raw.content.runtime.layout().layout().containing(&id("editor")).unwrap().location.zone(), Zone::Right);
        assert_eq!(raw.content.runtime.layout().inspect().revision, before.revision + 1);
        assert!(!raw.host.interaction().is_captured());
        assert_eq!(raw.content.commands.calls, 0);
    }
}

/// Every resize test starts from the actual mounted card, including a side
/// restored by one click on its full-width header.
fn raw_resize_card(scale: f32, restored_side: bool) -> (RawPanelHost, HostControlTarget) {
    let mut content = host_with_specs(|spec| {
        spec.max_size = Size { width: 360.0, height: 360.0 };
    });
    open_editor(&mut content);
    let key = editor_key(&content);
    content.probe.edit(&key, |state| state.draft = "card resize retains this transaction".into()).unwrap();
    if restored_side {
        content.dispatch(vec![Mutation::Collapse { section: section("editor-section"), collapsed: true }]).unwrap();
    } else {
        content
            .dispatch(vec![Mutation::Move {
                instance: id("editor"),
                destination: Destination::NewSection {
                    section: section("editor-section"),
                    index: 0,
                    location: SectionLocation::Floating { origin: Point { x: 200.0, y: 180.0 } },
                    size: Size { width: 280.0, height: 240.0 },
                },
            }])
            .unwrap();
    }
    let mut raw = RawPanelHost::new(content, scale);
    raw.prime_foreign_window();
    raw.frame(Vec::new(), false);
    raw.frame(Vec::new(), false);
    if restored_side {
        raw.click(HostControlTarget::Pointer(HostPointerTarget::Collapse { instance: id("editor") }), false);
    }
    raw.frame(Vec::new(), false);
    raw.frame(Vec::new(), false);
    let card = raw.host.frame().unwrap().layout.sections.iter().find(|card| card.section == section("editor-section")).unwrap();
    assert!(!card.peek);
    let target = raw
        .host
        .frame()
        .unwrap()
        .controls
        .iter()
        .find(|control| {
            matches!(&control.target,
                HostControlTarget::Pointer(HostPointerTarget::Resize { section: target, .. }) if *target == section("editor-section")
            )
        })
        .expect("mounted card must publish its resize control")
        .target
        .clone();
    (raw, target)
}

fn resize_card_delta(target: &HostControlTarget, extent: f32) -> egui::Vec2 {
    match target {
        HostControlTarget::Pointer(HostPointerTarget::Resize { edge, .. }) => match edge {
            HostResizeEdge::Left => egui::vec2(-extent, 0.0),
            HostResizeEdge::Right => egui::vec2(extent, 0.0),
            HostResizeEdge::Top => egui::vec2(0.0, -extent),
            HostResizeEdge::Bottom => egui::vec2(0.0, extent),
            HostResizeEdge::BottomRight => egui::vec2(extent, extent),
        },
        _ => panic!("expected an actual card resize control"),
    }
}

fn raw_control(raw: &RawPanelHost, target: &HostControlTarget) -> HostFrameControl {
    raw.host.frame().unwrap().controls.iter().find(|control| control.target == *target).unwrap().clone()
}

fn assert_host_landing(actual: &HostLayout, shown: &HostLayout) {
    assert_eq!(actual.sections, shown.sections);
    assert_eq!(actual.resolved.panels, shown.resolved.panels);
    assert_eq!(actual.extent, shown.extent);
    assert_eq!(actual.capacity, shown.capacity);
    assert_eq!(actual.render_domain, shown.render_domain);
    assert_eq!(actual.allocation_environment.scroll, shown.allocation_environment.scroll);
}

fn output_paints_rect(output: &egui::FullOutput, bounds: LogicalRect, fill: Option<egui::Color32>, stroke: Option<egui::Color32>) -> bool {
    fn matches(
        shape: &egui::Shape,
        transform: egui::emath::TSTransform,
        bounds: egui::Rect,
        fill: Option<egui::Color32>,
        stroke: Option<egui::Color32>,
    ) -> bool {
        match shape {
            egui::Shape::Rect(rect) => {
                transform * rect.rect == bounds && fill.is_none_or(|ink| rect.fill == ink) && stroke.is_none_or(|ink| rect.stroke.color == ink)
            }
            egui::Shape::Vec(shapes) => shapes.iter().any(|shape| matches(shape, transform, bounds, fill, stroke)),
            _ => false,
        }
    }
    output.shapes.iter().any(|shape| matches(&shape.shape, shape.transform_after_tessellation, egui_rect(bounds), fill, stroke))
}

#[test]
fn raw_split_preview_paints_the_exact_landing_and_changed_neighbours_before_one_commit() {
    for scale in [1.0, 2.0] {
        for direction in [Direction::Ltr, Direction::Rtl] {
            let mut content = PanelMountHarness::new(false).unwrap();
            open_editor(&mut content);
            content
                .dispatch(vec![
                    move_editor(Zone::Floating),
                    fixture_open("fixture.log", "target-0", "target-0", Zone::Right, 1).unwrap(),
                    fixture_open("fixture.log", "target-1", "target-1", Zone::Right, 2).unwrap(),
                ])
                .unwrap();
            let mut environment = content.runtime.environment().clone();
            environment.direction = direction;
            content.runtime.set_environment(environment, &mut content.local).unwrap();
            let mut raw = RawPanelHost::new(content, scale);
            raw.capacity.size = Size { width: 900.0, height: 600.0 };
            raw.frame(Vec::new(), false);
            raw.frame(Vec::new(), false);
            let initial = raw.host.frame().unwrap().layout.clone();
            let before = raw.content.runtime.layout().inspect();
            let at = raw.at(HostControlTarget::Pointer(HostPointerTarget::Header { instance: id("editor") }));
            let end = explicit_drop_point(
                &raw.content,
                &initial,
                &id("editor"),
                &HostDropTargetId::Relative { section: section("target-1"), placement: RelativePlacement::BeforeColumn },
            );
            raw.frame(vec![egui::Event::PointerMoved(at)], false);
            raw.pointer(at, true, false);
            let (moving, output) = raw.frame(vec![egui::Event::PointerMoved(egui::pos2(end.x, end.y))], false);
            let preview = moving.preview.unwrap();
            assert!(preview.rejection.is_none());
            let projected = preview.projected.unwrap();
            assert!(output_paints_rect(&output, preview.bounds, Some(solidcraft_theme::Tokens::get().form_primary.gamma_multiply(0.12)), None));
            let changed = projected
                .sections
                .iter()
                .filter(|card| {
                    card.instance != id("editor") && initial.sections.iter().any(|old| old.section == card.section && old.outer != card.outer)
                })
                .collect::<Vec<_>>();
            assert!(!changed.is_empty());
            for card in changed {
                assert!(output_paints_rect(&output, card.outer, None, Some(solidcraft_theme::Tokens::get().form_primary.gamma_multiply(0.7))));
            }
            assert_eq!(raw.content.runtime.layout().inspect(), before);
            assert_eq!(raw.host.frame().unwrap().layout.usable, initial.usable, "landing paint never commits the candidate usable region");
            raw.pointer(egui::pos2(end.x, end.y), false, true);
            raw.frame(Vec::new(), false);
            assert_host_landing(&raw.host.frame().unwrap().layout, &projected);
            assert_eq!(raw.content.runtime.layout().inspect().revision, before.revision + 1);
            raw.pointer(egui::pos2(end.x, end.y), false, true);
            assert_eq!(raw.content.runtime.layout().inspect().revision, before.revision + 1);
            assert_eq!(raw.content.commands.calls, 0);
        }
    }
}

#[test]
fn raw_join_activates_the_moved_tab_without_resizing_flash_in_floating_or_collapsed_header_hosts() {
    for scale in [1.0, 2.0] {
        for direction in [Direction::Ltr, Direction::Rtl] {
            for restored_side in [false, true] {
                let mut content = PanelMountHarness::new(false).unwrap();
                open_editor(&mut content);
                if restored_side {
                    content
                        .dispatch(vec![
                            move_editor(Zone::Right),
                            fixture_open("fixture.log", "landing", "landing-section", Zone::Left, 1).unwrap(),
                            Mutation::Collapse { section: section("landing-section"), collapsed: true },
                        ])
                        .unwrap();
                } else {
                    content
                        .dispatch(vec![
                            move_editor(Zone::Right),
                            fixture_open("fixture.log", "landing", "landing-section", Zone::Floating, 1).unwrap(),
                            Mutation::SetFloatingRect {
                                section: section("landing-section"),
                                origin: Point { x: 500.0, y: 300.0 },
                                size: Size { width: 280.0, height: 240.0 },
                            },
                        ])
                        .unwrap();
                }
                let mut environment = content.runtime.environment().clone();
                environment.direction = direction;
                content.runtime.set_environment(environment, &mut content.local).unwrap();
                let key = editor_key(&content);
                content.probe.edit(&key, |state| state.draft = "the visible landing reuses this editor".into()).unwrap();
                let mut raw = RawPanelHost::new(content, scale);
                raw.frame(Vec::new(), false);
                raw.frame(Vec::new(), false);
                let before = raw.content.runtime.layout().inspect();
                let retained = raw.content.probe.state(&key).unwrap();
                let controllers = raw.content.runtime.controller_count();
                let start = raw.at(HostControlTarget::Pointer(HostPointerTarget::Header { instance: id("editor") }));
                let end = explicit_drop_point(
                    &raw.content,
                    &raw.host.frame().unwrap().layout,
                    &id("editor"),
                    &HostDropTargetId::Tab { section: section("landing-section"), before: None },
                );
                raw.frame(vec![egui::Event::PointerMoved(start)], false);
                raw.pointer(start, true, false);
                let moving = raw.frame(vec![egui::Event::PointerMoved(egui::pos2(end.x, end.y))], false).0;
                let preview = moving.preview.unwrap();
                assert!(preview.rejection.is_none());
                let expected_mutations = vec![
                    Mutation::Move { instance: id("editor"), destination: Destination::JoinTab { section: section("landing-section"), index: 1 } },
                    Mutation::Activate { instance: id("editor") },
                ];
                assert_eq!(preview.action, Some(LayoutAction::Apply { expected_revision: Some(before.revision), mutations: expected_mutations }));
                let projected = preview.projected.unwrap();
                assert_eq!(projected.sections.iter().find(|card| card.section == section("landing-section")).unwrap().instance, id("editor"));
                assert_eq!(raw.content.runtime.layout().inspect(), before);
                raw.pointer(egui::pos2(end.x, end.y), false, false);
                let landing = raw.content.runtime.layout().layout().section(&section("landing-section")).unwrap();
                assert_eq!(landing.active, id("editor"));
                assert_eq!(landing.collapsed, restored_side);
                assert!(raw.content.runtime.layout().interaction().peek.is_none());
                let first_landing = raw.frame(Vec::new(), false).0;
                let card = first_landing.layout.sections.iter().find(|card| card.instance == id("editor")).unwrap();
                assert_eq!(card.outer, preview.bounds);
                assert!(!card.peek);
                assert!(
                    first_landing.layout.painted.contains(&card.outer),
                    "changing the active tab cannot recreate the section Area's invisible sizing pass"
                );
                assert!(first_landing.layout.input.contains(&card.outer));
                let surface = raw_control(&raw, &HostControlTarget::Pointer(HostPointerTarget::Surface { instance: id("editor") }));
                assert!(raw.ctx.read_response(surface.id).unwrap().enabled());
                assert_host_landing(&first_landing.layout, &projected);
                raw.pointer(egui::pos2(end.x, end.y), false, true);
                assert_eq!(raw.content.runtime.layout().inspect().revision, before.revision + 1);
                assert_eq!(raw.content.runtime.controller_count(), controllers);
                let after = raw.content.probe.state(&key).unwrap();
                assert_eq!(after.draft, retained.draft);
                assert!(after.transaction_live);
                assert_eq!(
                    (after.applies, after.cancels, after.finishes, after.destroys),
                    (retained.applies, retained.cancels, retained.finishes, retained.destroys)
                );
                assert_eq!(raw.content.commands.calls, 0);
            }
        }
    }
}

fn assert_no_host_mutation(raw: &RawPanelHost, before: &LayoutReply, native_commits: u64) {
    assert_eq!(&raw.content.runtime.layout().inspect(), before);
    assert_eq!(raw.content.native.commits, native_commits);
    assert_eq!(raw.content.commands.calls, 0);
    assert!(!raw.host.interaction().is_captured());
    assert!(raw.host.frame().unwrap().preview.is_none());
    assert!(
        !raw.host
            .frame()
            .unwrap()
            .events
            .iter()
            .any(|event| matches!(event, HostEvent::Committed { .. } | HostEvent::PreparedCommitted { .. } | HostEvent::ContentAction { .. }))
    );
}

fn assert_modal_preserves_painted_host_geometry(raw: &RawPanelHost) {
    let painted = &raw.painted_frames[0];
    let finished = &raw.finished_frames[0];
    assert_eq!(finished.layout.painted, painted.layout.painted);
    assert_eq!(finished.layout.input, painted.layout.input);
    assert_eq!(finished.layout.occlusion, painted.layout.occlusion);
    assert_eq!(finished.layout.reservations, painted.layout.reservations);
    assert_eq!(finished.layout.viewport, painted.layout.viewport);
    assert_eq!(finished.menu_bounds, painted.menu_bounds, "the current painted menu region is truthful even when its pending state is cancelled");
}

#[test]
fn late_shared_modal_backdrop_cancels_capture_while_ordinary_windows_preserve_real_release() {
    for scale in [1.0, 2.0] {
        for resize in [false, true] {
            // Ordinary foreign Window; late returning Modal; first sizing Modal;
            // already visible Modal before release of the old host capture.
            for mode in 0..4 {
                let (mut raw, target) = if resize {
                    raw_resize_card(scale, false)
                } else {
                    let mut content = PanelMountHarness::new(false).unwrap();
                    open_editor(&mut content);
                    let mut raw = RawPanelHost::new(content, scale);
                    raw.frame(Vec::new(), false);
                    raw.frame(Vec::new(), false);
                    (raw, HostControlTarget::Pointer(HostPointerTarget::Header { instance: id("editor") }))
                };
                raw.prime_foreign_window();
                if mode == 1 || mode == 3 {
                    raw.prime_modal();
                }
                let key = editor_key(&raw.content);
                raw.content.probe.edit(&key, |state| state.draft = "modal never cancels this live edit".into()).unwrap();
                let retained = raw.content.probe.state(&key).unwrap();
                let controllers = raw.content.runtime.controller_count();
                let before = raw.content.runtime.layout().inspect();
                let commits = raw.content.native.commits;
                let at = raw.at(target.clone());
                raw.frame(vec![egui::Event::PointerMoved(at)], false);
                raw.pointer(at, true, false);
                assert!(raw.host.interaction().is_captured());
                let end = if resize { at + resize_card_delta(&target, 36.0) } else { egui::pos2(raw.capacity.size.width - 1.0, 550.0) };
                let preview = raw.frame(vec![egui::Event::PointerMoved(end)], false).0.preview.unwrap();
                assert!(preview.rejection.is_none() && preview.action.is_some());
                assert_eq!(raw.content.runtime.layout().inspect(), before);
                if mode == 0 {
                    raw.foreign_cover = Some(egui::Rect::from_center_size(end, egui::vec2(100.0, 80.0)));
                } else {
                    raw.late_modal = true;
                }
                if mode == 3 {
                    raw.frame(Vec::new(), false);
                    assert_no_host_mutation(&raw, &before, commits);
                    assert_modal_preserves_painted_host_geometry(&raw);
                }
                let updates = raw.content.probe.state(&key).unwrap().updates;
                let mut events = vec![egui::Event::PointerMoved(end), button_event(end, false)];
                if mode == 3 {
                    events.push(host_key(egui::Key::Escape, true));
                }
                raw.frame(events, true);
                assert_eq!(
                    raw.content.probe.state(&key).unwrap().updates,
                    updates + 1,
                    "modal paint and extra egui passes cannot add a service tick"
                );
                if mode == 0 {
                    assert_eq!(raw.content.runtime.layout().inspect().revision, before.revision + 1);
                    assert_eq!(raw.content.native.commits, commits + 1);
                    assert!(
                        raw.host
                            .frame()
                            .unwrap()
                            .events
                            .iter()
                            .any(|event| matches!(event, HostEvent::PreparedCommitted { reply } if reply.revision == before.revision + 1))
                    );
                } else {
                    assert_no_host_mutation(&raw, &before, commits);
                    assert_modal_preserves_painted_host_geometry(&raw);
                    assert!(!raw.modal_observations.is_empty());
                    for modal in &raw.modal_observations {
                        assert!(!modal.card.contains(end), "the modal card itself is away from the host release");
                        assert!(modal.backdrop.contains(end), "its actual shared backdrop supplies modality");
                    }
                    if mode == 2 {
                        assert!(!raw.modal_observations[0].visible, "the first sizing pass still blocks by actual modal show intent");
                    }
                    if mode == 1 || mode == 3 {
                        assert!(raw.modal_observations[0].visible);
                    }
                    if mode == 3 {
                        assert!(
                            raw.modal_observations.iter().any(|modal| modal.close_requested),
                            "a warmed active modal receives Escape after host finish"
                        );
                    }
                }
                raw.late_modal = false;
                raw.foreign_cover = None;
                raw.frame(vec![host_key(egui::Key::Escape, false)], false);
                raw.frame(Vec::new(), false);
                raw.pointer(end, false, true);
                raw.frame(Vec::new(), true);
                if mode == 0 {
                    assert_eq!(raw.content.runtime.layout().inspect().revision, before.revision + 1);
                } else {
                    assert_no_host_mutation(&raw, &before, commits);
                }
                let after = raw.content.probe.state(&key).unwrap();
                assert_eq!(after.draft, retained.draft);
                assert!(after.transaction_live);
                assert_eq!(
                    (after.cancels, after.applies, after.finishes, after.destroys),
                    (retained.cancels, retained.applies, retained.finishes, retained.destroys)
                );
                if mode != 0 {
                    assert_eq!(after.reparents, retained.reparents);
                }
                assert_eq!(raw.content.runtime.controller_count(), controllers);
                assert!(raw.content.native.hosts.is_empty());
                assert_eq!(raw.content.commands.calls, 0);
            }
        }
    }
}

#[test]
fn late_shared_modal_discards_pointer_and_keyboard_intents_without_latent_actions() {
    for scale in [1.0, 2.0] {
        for keyboard in [false, true] {
            for action in 0..3 {
                let mut content = PanelMountHarness::new(false).unwrap();
                open_editor(&mut content);
                let mut raw = RawPanelHost::new(content, scale);
                raw.frame(Vec::new(), false);
                raw.frame(Vec::new(), false);
                raw.prime_modal();
                let key = editor_key(&raw.content);
                raw.content.probe.edit(&key, |state| state.draft = "early host intent cannot cross a late modal".into()).unwrap();
                if action == 2 {
                    raw.menu(id("editor"));
                }
                let target = match action {
                    0 => HostControlTarget::Pointer(HostPointerTarget::Menu { instance: id("editor") }),
                    1 => HostControlTarget::Content { instance: id("editor"), action: ControlId::new("apply").unwrap() },
                    _ => HostControlTarget::Shared { instance: id("editor"), action: HostSharedAction::Move(Zone::Right) },
                };
                let widget = raw_control(&raw, &target);
                assert!(widget.enabled);
                let at = raw.at(target);
                if keyboard {
                    raw.ctx.memory_mut(|memory| memory.request_focus(widget.id));
                    raw.frame(Vec::new(), false);
                    assert_eq!(raw.ctx.memory(|memory| memory.focused()), Some(widget.id));
                } else {
                    raw.frame(vec![egui::Event::PointerMoved(at)], false);
                    raw.pointer(at, true, false);
                }
                let before = raw.content.runtime.layout().inspect();
                let commits = raw.content.native.commits;
                let retained = raw.content.probe.state(&key).unwrap();
                let controllers = raw.content.runtime.controller_count();
                raw.late_modal = true;
                let events =
                    if keyboard { vec![host_key(egui::Key::Enter, true)] } else { vec![egui::Event::PointerMoved(at), button_event(at, false)] };
                raw.frame(events, true);
                assert!(
                    raw.clicked_widgets.contains(&widget.id),
                    "the earlier real response must queue an intent before the late modal blocks finish"
                );
                assert_no_host_mutation(&raw, &before, commits);
                assert_modal_preserves_painted_host_geometry(&raw);
                assert!(raw.modal_observations[0].visible);
                assert!(!raw.modal_observations[0].card.contains(at));
                assert!(raw.modal_observations[0].backdrop.contains(at));
                raw.late_modal = false;
                raw.frame(vec![host_key(egui::Key::Enter, false)], false);
                raw.frame(Vec::new(), false);
                raw.frame(Vec::new(), true);
                assert_no_popup(raw.host.frame().unwrap());
                assert_no_host_mutation(&raw, &before, commits);
                let after = raw.content.probe.state(&key).unwrap();
                assert_eq!(after.draft, retained.draft);
                assert_eq!(
                    (after.reparents, after.cancels, after.applies, after.finishes, after.destroys),
                    (retained.reparents, retained.cancels, retained.applies, retained.finishes, retained.destroys)
                );
                assert!(after.transaction_live);
                assert_eq!(raw.content.runtime.controller_count(), controllers);
            }
        }
    }
}

#[test]
fn shared_modal_receives_reserved_escape_once_and_keeps_the_restored_header_and_live_edit() {
    for scale in [1.0, 2.0] {
        for restored_side in [false, true] {
            let mut content = PanelMountHarness::new(false).unwrap();
            open_editor(&mut content);
            let mut raw = RawPanelHost::new(content, scale);
            raw.frame(Vec::new(), false);
            raw.frame(Vec::new(), false);
            raw.prime_modal();
            let key = editor_key(&raw.content);
            raw.content.probe.edit(&key, |state| state.draft = "Escape belongs to the modal, not this transaction".into()).unwrap();
            // egui derives repeat from held-key state. Establish the first
            // keydown before opening a host transient, then test a real repeat.
            raw.frame(vec![host_key(egui::Key::Escape, true)], false);
            if restored_side {
                raw.content.dispatch(vec![Mutation::Collapse { section: section("editor-section"), collapsed: true }]).unwrap();
                raw.frame(Vec::new(), false);
                raw.click(HostControlTarget::Pointer(HostPointerTarget::Collapse { instance: id("editor") }), false);
                raw.frame(Vec::new(), false);
                raw.frame(Vec::new(), false);
                assert!(!raw.content.runtime.layout().layout().containing(&id("editor")).unwrap().collapsed);
                // Expanding restores content without a transient. Reserve
                // Escape for a real capture on the restored header instead.
                let at = raw.at(HostControlTarget::Pointer(HostPointerTarget::Header { instance: id("editor") }));
                raw.pointer(at, true, false);
                assert!(raw.host.interaction().is_captured());
            } else {
                raw.menu(id("editor"));
            }
            let before = raw.content.runtime.layout().inspect();
            let commits = raw.content.native.commits;
            let retained = raw.content.probe.state(&key).unwrap();
            let escape = egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: Some(egui::Key::Escape),
                pressed: true,
                repeat: true,
                modifiers: egui::Modifiers::NONE,
            };
            raw.reserve_escape_for_host = true;
            raw.late_modal = true;
            let mut events = vec![escape.clone()];
            if restored_side {
                let at = raw.at(HostControlTarget::Pointer(HostPointerTarget::Header { instance: id("editor") }));
                events.push(button_event(at, false));
            }
            raw.frame(events, false);
            assert_eq!(raw.escape_reservations, 1, "match the shell reserving Escape before a late modal appears");
            assert_eq!(
                raw.modal_observations[0].escape_events,
                vec![escape.clone()],
                "transfer the original event without losing its physical/repeat metadata"
            );
            assert_no_host_mutation(&raw, &before, commits);
            assert_modal_preserves_painted_host_geometry(&raw);
            raw.frame(vec![host_key(egui::Key::Escape, false)], true);
            assert_eq!(raw.escape_reservations, 0);
            assert!(raw.modal_observations.iter().all(|modal| modal.escape_events.is_empty()));
            assert_no_popup(raw.host.frame().unwrap());
            let mut first_press = escape.clone();
            if let egui::Event::Key { repeat, .. } = &mut first_press {
                *repeat = false;
            }
            raw.frame(vec![first_press.clone()], true);
            assert_eq!(raw.escape_reservations, 0, "an active modal prevents host reservation");
            assert_eq!(raw.modal_observations[0].escape_events, vec![first_press]);
            assert!(
                raw.modal_observations.iter().any(|modal| modal.close_requested),
                "a warmed active modal handles Escape normally after host finish"
            );
            assert!(raw.modal_observations.iter().all(|modal| modal.escape_events.len() <= 1));
            raw.late_modal = false;
            raw.frame(vec![host_key(egui::Key::Escape, false)], false);
            raw.frame(Vec::new(), true);
            assert_no_host_mutation(&raw, &before, commits);
            assert_no_popup(raw.host.frame().unwrap());
            let after = raw.content.probe.state(&key).unwrap();
            assert_eq!(after.draft, retained.draft);
            assert!(after.transaction_live);
            assert_eq!(
                (after.reparents, after.cancels, after.applies, after.finishes, after.destroys),
                (retained.reparents, retained.cancels, retained.applies, retained.finishes, retained.destroys)
            );
        }
    }
}

#[test]
fn raw_floating_and_restored_side_resize_widgets_own_the_card_layer_at_both_scales() {
    for scale in [1.0, 2.0] {
        for restored_side in [false, true] {
            let (mut raw, target) = raw_resize_card(scale, restored_side);
            let key = editor_key(&raw.content);
            let retained = raw.content.probe.state(&key).unwrap();
            let controllers = raw.content.runtime.controller_count();
            let native_commits = raw.content.native.commits;
            let original = raw_control(&raw, &target);
            let native_hosts = raw.content.native.hosts.clone();
            for extent in [36.0, 84.0, -260.0] {
                let at = raw.at(target.clone());
                let (_, output) = raw.frame(vec![egui::Event::PointerMoved(at)], false);
                let control = raw_control(&raw, &target);
                let response = raw.ctx.read_response(control.id).unwrap();
                let surface = raw_control(&raw, &HostControlTarget::Pointer(HostPointerTarget::Surface { instance: id("editor") }));
                assert!(response.enabled());
                assert_eq!(response.rect, egui_rect(control.bounds));
                assert_eq!(response.layer_id, raw.ctx.read_response(surface.id).unwrap().layer_id);
                assert_eq!(raw.ctx.layer_id_at(at), Some(response.layer_id));
                assert_eq!(Some(response.layer_id) == raw.root_layer, restored_side, "restored side cards use the root; floating uses its Area");
                assert_eq!(control.id, original.id);
                assert_eq!(control.accessible_name, original.accessible_name);
                assert!(
                    output
                        .platform_output
                        .accesskit_update
                        .as_ref()
                        .unwrap()
                        .nodes
                        .iter()
                        .any(|(_, node)| node.label() == Some(control.accessible_name.as_str()))
                );
                let before = raw.content.runtime.layout().inspect();
                raw.pointer(at, true, false);
                assert!(raw.host.interaction().is_captured());
                let end = at + resize_card_delta(&target, extent);
                let preview = raw.frame(vec![egui::Event::PointerMoved(end)], false).0.preview.unwrap();
                assert!(preview.rejection.is_none());
                assert!(preview.action.is_some(), "motion reaches a different registered bound");
                assert_eq!(raw.content.runtime.layout().inspect(), before);
                raw.pointer(end, false, true);
                let committed = raw
                    .host
                    .frame()
                    .unwrap()
                    .events
                    .iter()
                    .filter_map(|event| match event {
                        HostEvent::PreparedCommitted { reply } => Some(reply),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                assert_eq!(committed.len(), 1);
                assert_eq!(committed[0].revision, before.revision + 1);
                assert!(raw.host.frame().unwrap().events.iter().all(|event| match event {
                    HostEvent::PreparedCommitted { reply } => reply.effects.is_empty(),
                    _ => true,
                }));
                assert_eq!(raw.content.runtime.layout().inspect().revision, before.revision + 1);
                raw.frame(Vec::new(), false);
                let card = raw.host.frame().unwrap().layout.sections.iter().find(|card| card.section == section("editor-section")).unwrap();
                assert_eq!(card.outer, preview.bounds, "the visible committed card equals the final preview");
                if extent == 84.0 {
                    assert_eq!(card.outer.size.width, 360.0);
                    if !restored_side {
                        assert_eq!(card.outer.size.height, 360.0);
                    }
                }
                if extent == -260.0 {
                    assert_eq!(card.outer.size.width, 160.0);
                    if !restored_side {
                        assert_eq!(card.outer.size.height, 120.0);
                    }
                }
                let resized = raw_control(&raw, &target);
                assert_eq!(resized.id, original.id);
                assert_ne!(resized.bounds, control.bounds);
                assert_eq!(raw.ctx.read_response(resized.id).unwrap().rect, egui_rect(resized.bounds));
                raw.pointer(end, false, true);
                assert_eq!(raw.content.runtime.layout().inspect().revision, before.revision + 1);
            }
            let after = raw.content.probe.state(&key).unwrap();
            assert_eq!(after.draft, retained.draft);
            assert_eq!(
                (after.reparents, after.cancels, after.applies, after.finishes, after.destroys),
                (retained.reparents, retained.cancels, retained.applies, retained.finishes, retained.destroys)
            );
            assert_eq!(raw.content.runtime.controller_count(), controllers);
            assert_eq!(raw.content.native.commits, native_commits + 3, "one adapter commit per accepted resize, with no native effects");
            assert_eq!(raw.content.native.hosts, native_hosts);
            assert!(after.transaction_live);
            assert_eq!(raw.content.commands.calls, 0);
        }
    }
}

#[test]
fn raw_card_resize_rejects_foreign_initial_presses_but_keeps_real_capture() {
    for scale in [1.0, 2.0] {
        for restored_side in [false, true] {
            for warm_cover in [false, true] {
                let (mut raw, target) = raw_resize_card(scale, restored_side);
                let at = raw.at(target.clone());
                raw.frame(vec![egui::Event::PointerMoved(at)], false);
                let source = raw_control(&raw, &target);
                let before = raw.content.runtime.layout().inspect();
                let native_commits = raw.content.native.commits;
                raw.foreign_cover = Some(egui::Rect::from_center_size(at, egui::vec2(100.0, 100.0)));
                if warm_cover {
                    raw.frame(Vec::new(), false);
                }
                raw.pointer(at, true, true);
                assert_ne!(raw.ctx.layer_id_at(at), Some(raw.ctx.read_response(source.id).unwrap().layer_id));
                assert!(!raw.host.interaction().is_captured());
                let end = at + resize_card_delta(&target, 32.0);
                raw.frame(vec![egui::Event::PointerMoved(end)], false);
                raw.pointer(end, false, true);
                assert_no_host_mutation(&raw, &before, native_commits);
                raw.foreign_cover = None;
                raw.frame(Vec::new(), false);
                raw.frame(Vec::new(), false);
                raw.frame(vec![egui::Event::PointerMoved(at)], false);
                raw.pointer(at, true, false);
                assert!(raw.host.interaction().is_captured());
                let outside = egui::pos2(raw.capacity.size.width + 80.0, raw.capacity.size.height + 80.0);
                raw.frame(vec![egui::Event::PointerMoved(outside)], false);
                assert!(raw.host.interaction().is_captured());
                let preview = raw.frame(vec![egui::Event::PointerMoved(end)], false).0.preview.unwrap();
                raw.foreign_cover = Some(egui::Rect::from_center_size(end, egui::vec2(100.0, 100.0)));
                raw.pointer(end, false, true);
                assert_eq!(raw.content.runtime.layout().inspect().revision, before.revision + 1);
                assert!(!raw.host.interaction().is_captured());
                assert!(
                    raw.host
                        .frame()
                        .unwrap()
                        .events
                        .iter()
                        .any(|event| matches!(event, HostEvent::PreparedCommitted { reply } if reply.revision == before.revision + 1))
                );
                raw.frame(Vec::new(), false);
                assert_eq!(
                    raw.host.frame().unwrap().layout.sections.iter().find(|card| card.section == section("editor-section")).unwrap().outer,
                    preview.bounds
                );
                raw.pointer(end, false, true);
                assert_eq!(raw.content.runtime.layout().inspect().revision, before.revision + 1);
                assert_eq!(raw.content.native.commits, native_commits + 1);
                assert!(raw.content.native.hosts.is_empty());
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum PopupFixture {
    Menu,
    Header,
    Tab,
    CollapsedHeader,
    Tabs,
    ScrolledMenu,
    CompactTabs,
}

fn raw_popup_fixture(scale: f32, fixture: PopupFixture) -> (RawPanelHost, HostControlTarget, egui::PointerButton) {
    let mut content = host_logs(if matches!(fixture, PopupFixture::ScrolledMenu) { 24 } else { 1 }, Zone::Left);
    if matches!(fixture, PopupFixture::Tab | PopupFixture::Tabs | PopupFixture::CompactTabs) {
        for index in 1..MAX_TABS {
            content
                .dispatch(vec![Mutation::Open {
                    kind: PanelKind::new("fixture.log").unwrap(),
                    instance: id(&format!("tab-{index}")),
                    destination: Some(Destination::JoinTab { section: section("section-0"), index }),
                }])
                .unwrap();
        }
        content
            .dispatch(vec![
                Mutation::Activate { instance: id("log-0") },
                Mutation::SetSize {
                    section: section("section-0"),
                    size: Size { width: if matches!(fixture, PopupFixture::CompactTabs) { 240.0 } else { 160.0 }, height: 240.0 },
                },
            ])
            .unwrap();
    }
    if matches!(fixture, PopupFixture::CollapsedHeader | PopupFixture::ScrolledMenu) {
        content
            .dispatch(
                content
                    .runtime
                    .layout()
                    .layout()
                    .sections
                    .iter()
                    .map(|section| Mutation::Collapse { section: section.id.clone(), collapsed: true })
                    .collect(),
            )
            .unwrap();
    }
    let pointer = match fixture {
        PopupFixture::Menu => HostPointerTarget::Menu { instance: id("log-0") },
        PopupFixture::Header | PopupFixture::CollapsedHeader => HostPointerTarget::Header { instance: id("log-0") },
        PopupFixture::Tab => HostPointerTarget::Tab { instance: id("log-0") },
        PopupFixture::Tabs | PopupFixture::CompactTabs => HostPointerTarget::TabOverflow { section: section("section-0") },
        PopupFixture::ScrolledMenu => HostPointerTarget::Menu { instance: id("log-23") },
    };
    let mut raw = RawPanelHost::new(content, scale);
    if matches!(fixture, PopupFixture::ScrolledMenu) {
        raw.capacity.size = Size { width: 480.0, height: 320.0 };
    }
    raw.prime_foreign_window();
    raw.frame(Vec::new(), false);
    raw.frame(Vec::new(), false);
    if matches!(fixture, PopupFixture::ScrolledMenu) {
        let bounds = raw
            .host
            .frame()
            .unwrap()
            .controls
            .iter()
            .find(|control| control.target == HostControlTarget::Scroll { horizontal: false })
            .unwrap()
            .bounds;
        let at = egui_rect(bounds).center_bottom() - egui::vec2(0.0, 1.0);
        raw.pointer(at, true, false);
        raw.pointer(at, false, false);
        raw.frame(Vec::new(), false);
    }
    let target = HostControlTarget::Pointer(pointer);
    assert!(raw_control(&raw, &target).enabled);
    let button = if matches!(fixture, PopupFixture::Header | PopupFixture::Tab | PopupFixture::CollapsedHeader) {
        egui::PointerButton::Secondary
    } else {
        egui::PointerButton::Primary
    };
    (raw, target, button)
}
fn popup_button(at: egui::Pos2, button: egui::PointerButton, pressed: bool) -> egui::Event {
    egui::Event::PointerButton { pos: at, button, pressed, modifiers: egui::Modifiers::NONE }
}

fn host_key(key: egui::Key, pressed: bool) -> egui::Event {
    egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: egui::Modifiers::NONE }
}

fn assert_no_popup(frame: &HostFrame) {
    assert!(frame.menu_bounds.is_none());
    assert!(!frame.controls.iter().any(|control| matches!(control.target, HostControlTarget::PanelMenu { .. } | HostControlTarget::Shared { .. })));
}

fn open_root_popup(raw: &mut RawPanelHost, source: &HostControlTarget, button: egui::PointerButton, multipass: bool) {
    let previously_painted = raw.painted_menu;
    let at = raw.at(source.clone());
    raw.frame(vec![egui::Event::PointerMoved(at)], false);
    raw.frame(vec![popup_button(at, button, true)], false);
    raw.frame(vec![popup_button(at, button, false)], multipass);
    assert_no_popup(&raw.painted_frames[0]);
    assert_no_popup(&raw.finished_frames[0]);
    raw.frame(Vec::new(), false);
    if !multipass && !previously_painted {
        assert_no_popup(raw.host.frame().unwrap());
    }
    raw.frame(Vec::new(), false);
    assert!(raw.host.frame().unwrap().menu_bounds.is_some(), "accepted popup is published after it actually paints");
}

#[test]
fn late_first_appearance_window_blocks_all_root_popup_open_intents() {
    for scale in [1.0, 2.0] {
        for fixture in [
            PopupFixture::Menu,
            PopupFixture::Header,
            PopupFixture::Tab,
            PopupFixture::CollapsedHeader,
            PopupFixture::Tabs,
            PopupFixture::ScrolledMenu,
            PopupFixture::CompactTabs,
        ] {
            for same_frame in [false, true] {
                let (mut raw, source, button) = raw_popup_fixture(scale, fixture);
                let at = raw.at(source.clone());
                raw.frame(vec![egui::Event::PointerMoved(at)], false);
                let source_id = raw_control(&raw, &source).id;
                let before = raw.content.runtime.layout().inspect();
                let commits = raw.content.native.commits;
                if !same_frame {
                    raw.frame(vec![popup_button(at, button, true)], false);
                }
                raw.foreign_cover = Some(egui::Rect::from_center_size(at, egui::vec2(100.0, 80.0)));
                let mut events = vec![egui::Event::PointerMoved(at)];
                if same_frame {
                    events.push(popup_button(at, button, true));
                }
                events.push(popup_button(at, button, false));
                raw.frame(events, true);
                assert_ne!(raw.ctx.layer_id_at(at), Some(raw.ctx.read_response(source_id).unwrap().layer_id));
                for painted in &raw.painted_frames {
                    assert_no_popup(painted);
                }
                for finished in &raw.finished_frames {
                    assert_no_popup(finished);
                }
                assert_no_host_mutation(&raw, &before, commits);
                raw.foreign_cover = None;
                raw.frame(Vec::new(), true);
                assert_no_popup(raw.host.frame().unwrap());
                raw.frame(vec![popup_button(at, button, false)], true);
                assert_no_popup(raw.host.frame().unwrap());
                assert_no_host_mutation(&raw, &before, commits);

                // A fresh real source press after the covered gesture succeeds.
                raw.frame(Vec::new(), false);
                open_root_popup(&mut raw, &source, button, true);
                let bounds = raw.host.frame().unwrap().menu_bounds;
                raw.frame(Vec::new(), true);
                assert_eq!(raw.host.frame().unwrap().menu_bounds, bounds);
                raw.frame(vec![host_key(egui::Key::Escape, true)], true);
                raw.frame(vec![host_key(egui::Key::Escape, false)], false);
                assert_no_popup(raw.host.frame().unwrap());
                open_root_popup(&mut raw, &source, button, false);
                let outside = egui::pos2(-20.0, -20.0);
                raw.pointer(outside, true, true);
                raw.pointer(outside, false, false);
                raw.frame(Vec::new(), false);
                assert_no_popup(raw.host.frame().unwrap());
            }
            // Header/tab/collapsed-header context menus use secondary pointer activation;
            // the primary menu and overflow buttons also expose keyboard clicks.
            if matches!(fixture, PopupFixture::Menu | PopupFixture::Tabs | PopupFixture::ScrolledMenu | PopupFixture::CompactTabs) {
                for key in [egui::Key::Enter, egui::Key::Space] {
                    for late_focus in [false, true] {
                        let (mut raw, source, _) = raw_popup_fixture(scale, fixture);
                        let control = raw_control(&raw, &source);
                        raw.ctx.memory_mut(|memory| memory.request_focus(control.id));
                        raw.frame(Vec::new(), false);
                        if late_focus {
                            raw.foreign_cover = Some(egui::Rect::from_min_size(
                                egui::pos2(raw.capacity.size.width - 100.0, raw.capacity.size.height - 100.0),
                                egui::vec2(80.0, 80.0),
                            ));
                            raw.foreign_takes_focus = true;
                        }
                        raw.frame(vec![host_key(key, true)], true);
                        assert_no_popup(&raw.painted_frames[0]);
                        assert_no_popup(&raw.finished_frames[0]);
                        raw.foreign_cover = None;
                        raw.foreign_takes_focus = false;
                        raw.frame(vec![host_key(key, false)], false);
                        raw.frame(Vec::new(), true);
                        if late_focus {
                            assert_no_popup(raw.host.frame().unwrap());
                        } else {
                            assert!(raw.host.frame().unwrap().menu_bounds.is_some());
                        }
                        assert_eq!(raw.content.commands.calls, 0);
                    }
                }
            }
        }
    }
    // Keyboard header restoration must recheck actual current focus too.
    for scale in [1.0, 2.0] {
        for late_focus in [false, true] {
            let (mut raw, _, _) = raw_popup_fixture(scale, PopupFixture::CollapsedHeader);
            let target = HostControlTarget::Pointer(HostPointerTarget::Collapse { instance: id("log-0") });
            let source_id = raw_control(&raw, &target).id;
            raw.ctx.memory_mut(|memory| memory.request_focus(source_id));
            raw.frame(Vec::new(), false);
            let before = raw.content.runtime.layout().inspect();
            if late_focus {
                raw.foreign_cover = Some(egui::Rect::from_min_size(egui::pos2(800.0, 600.0), egui::vec2(80.0, 80.0)));
                raw.foreign_takes_focus = true;
            }
            raw.frame(vec![host_key(egui::Key::Enter, true)], true);
            raw.foreign_cover = None;
            raw.foreign_takes_focus = false;
            raw.frame(vec![host_key(egui::Key::Enter, false)], true);
            if late_focus {
                assert_eq!(raw.content.runtime.layout().inspect(), before);
            } else {
                assert!(!raw.content.runtime.layout().layout().containing(&id("log-0")).unwrap().collapsed);
            }
        }
    }
}

#[test]
fn late_first_appearance_window_blocks_nested_popup_navigation() {
    for scale in [1.0, 2.0] {
        for fixture in [PopupFixture::Tabs, PopupFixture::CompactTabs] {
            for same_frame in [false, true] {
                let (mut raw, source, button) = raw_popup_fixture(scale, fixture);
                open_root_popup(&mut raw, &source, button, false);
                // Both Areas can be raised by this press frame. Warm the
                // foreign Area after the popup so their actual stable pair
                // order puts the returning window above the popup as well.
                raw.prime_foreign_window();
                raw.frame(Vec::new(), false);
                let target = HostControlTarget::PanelMenu { instance: id("log-0") };
                let row = raw_control(&raw, &target);
                let original_layer = raw.ctx.read_response(row.id).unwrap().layer_id;
                let at = raw.at(target.clone());
                let original_bounds = raw.host.frame().unwrap().menu_bounds;
                let original_rows = raw
                    .host
                    .frame()
                    .unwrap()
                    .controls
                    .iter()
                    .filter(|control| matches!(control.target, HostControlTarget::PanelMenu { .. }))
                    .cloned()
                    .collect::<Vec<_>>();
                let before = raw.content.runtime.layout().inspect();
                let commits = raw.content.native.commits;
                raw.frame(vec![egui::Event::PointerMoved(at)], false);
                if !same_frame {
                    raw.pointer(at, true, false);
                }
                raw.foreign_cover = Some(egui::Rect::from_center_size(at, egui::vec2(100.0, 40.0)));
                let mut events = vec![egui::Event::PointerMoved(at)];
                if same_frame {
                    events.push(button_event(at, true));
                }
                events.push(button_event(at, false));
                raw.frame(events, true);
                let foreign = raw.foreign_responses.first().expect("the late real Window must actually render");
                assert!(foreign.enabled() && foreign.interact_rect.contains(at));
                assert_ne!(foreign.layer_id, raw.ctx.read_response(row.id).unwrap().layer_id);
                assert_eq!(raw.ctx.layer_id_at(at), Some(foreign.layer_id), "the actual enabled foreign Window must own the covered point");
                for painted in &raw.painted_frames {
                    assert_eq!(painted.menu_bounds, original_bounds);
                }
                for finished in &raw.finished_frames {
                    assert_eq!(finished.menu_bounds, original_bounds);
                }
                raw.foreign_cover = None;
                let mut actual_rows = std::collections::HashMap::new();
                let _ = raw.frame_late(Vec::new(), true, |_, ctx| {
                    // Snapshot the current painted pass before egui swaps its
                    // response maps at end_pass; outside reads can be stale.
                    actual_rows.clear();
                    for original in &original_rows {
                        if let Some(response) = ctx.read_response(original.id) {
                            actual_rows.insert(original.id, response);
                        }
                    }
                });
                assert_eq!(raw.host.frame().unwrap().menu_bounds, original_bounds);
                let unchanged_source = raw_control(&raw, &target);
                assert_eq!(unchanged_source.id, row.id, "blocked input must retain the exact root menu source");
                assert_eq!(actual_rows.get(&unchanged_source.id).unwrap().layer_id, original_layer);
                for original in &original_rows {
                    if let Some(current) = raw.host.frame().unwrap().controls.iter().find(|control| control.target == original.target) {
                        assert_eq!(current.id, original.id);
                    } else {
                        // Text/scroll layout can move a partially visible last
                        // row below the clip. It may disappear from published
                        // controls only if its real response is now clipped.
                        let clipped = actual_rows.get(&original.id).expect("the root menu still creates its clipped row in the current pass");
                        assert!(
                            !clipped.interact_rect.intersect(egui_rect(original_bounds.unwrap())).is_positive(),
                            "blocked input cannot remove a still-visible root menu row: {:?}",
                            original.target
                        );
                    }
                }
                assert!(
                    !raw.host.frame().unwrap().controls.iter().any(|control| matches!(
                        control.target,
                        HostControlTarget::Shared { action: HostSharedAction::Move(_) | HostSharedAction::Close | HostSharedAction::Collapse, .. }
                    )),
                    "a list may expose Focus, but must not have navigated into panel actions"
                );
                assert_no_host_mutation(&raw, &before, commits);
                raw.frame(Vec::new(), false);
                raw.click(target.clone(), true);
                assert!(raw.painted_frames[0].controls.iter().any(|control| control.target == target));
                raw.frame(Vec::new(), false);
                raw.frame(Vec::new(), false);
                assert!(
                    raw.host
                        .frame()
                        .unwrap()
                        .controls
                        .iter()
                        .any(|control| matches!(control.target, HostControlTarget::Shared { action: HostSharedAction::Move(_), .. }))
                );
                assert!(!raw.host.frame().unwrap().controls.iter().any(|control| control.target == target));
                raw.frame(Vec::new(), true);
                assert_eq!(raw.content.runtime.layout().inspect(), before);
            }
            for cancel in [0, 1, 2] {
                let (mut raw, source, button) = raw_popup_fixture(scale, fixture);
                open_root_popup(&mut raw, &source, button, false);
                raw.frame(Vec::new(), false);
                let target = HostControlTarget::PanelMenu { instance: id("log-0") };
                let row = raw_control(&raw, &target);
                raw.ctx.memory_mut(|memory| memory.request_focus(row.id));
                raw.frame(Vec::new(), false);
                let mut events = vec![host_key(egui::Key::Enter, true)];
                if cancel == 1 {
                    events.push(host_key(egui::Key::Escape, true));
                }
                if cancel == 2 {
                    raw.content.tick().unwrap();
                    raw.host.capture_metrics(&raw.content.runtime, &raw.content.models).unwrap();
                    raw.host.prepare(&raw.content.runtime, raw.capacity).unwrap();
                    raw.time += 1.0 / 60.0;
                    let mut output = raw.ctx.run_ui(
                        egui::RawInput {
                            screen_rect: Some(egui_rect(raw.capacity)),
                            events,
                            time: Some(raw.time),
                            focused: true,
                            ..Default::default()
                        },
                        |ui| {
                            let h = &mut raw.content;
                            let mut services = ContentServices { models: &h.models, commands: &mut h.commands, local: &mut h.local };
                            raw.host.show_deferred(ui, &mut h.runtime, &mut h.native, &mut services).unwrap();
                            let mut environment = h.runtime.environment().clone();
                            environment.document = Some(DocumentId::new("new-popup-context").unwrap());
                            h.runtime.set_environment(environment, services.local).unwrap();
                            assert_eq!(
                                raw.host.finish(ui, &mut h.runtime, &mut h.native, &mut services, &mut h.catalogue).unwrap_err().rule,
                                Rule::Placement
                            );
                        },
                    );
                    output.textures_delta.clear();
                    assert!(raw.host.frame().is_none(), "pending navigation is invalidated with its owner scope");
                } else {
                    raw.frame(events, true);
                }
                raw.frame(vec![host_key(egui::Key::Enter, false), host_key(egui::Key::Escape, false)], true);
                if cancel == 0 {
                    assert!(raw.host.frame().unwrap().controls.iter().any(|control| matches!(control.target, HostControlTarget::Shared { .. })));
                    let outside = egui::pos2(-20.0, -20.0);
                    raw.pointer(outside, true, true);
                    raw.pointer(outside, false, false);
                    raw.frame(Vec::new(), true);
                }
                assert_no_popup(raw.host.frame().unwrap());
                assert_eq!(raw.content.commands.calls, 0);
            }
        }
    }
}

#[test]
fn tick_failed_document_refresh_preserves_old_owners_and_attempts_services_once() {
    for scale in [1.0, 2.0] {
        for captured in [false, true] {
            let (mut h, probe, trace) = boundary_app();
            h.ctx.set_pixels_per_point(scale);
            h.app
                .dispatch_panel_layout(transaction(vec![fixture_open("fixture.log", "zz-workspace", "workspace-section", Zone::Right, 0).unwrap()]))
                .unwrap();
            h.frames(2);
            let old_key = app_editor_key(&h.app);
            let environment = h.app.panel_runtime().unwrap().environment().clone();
            let workspace_key = ControllerKey { instance: id("zz-workspace"), owner: ContentOwner::Workspace(environment.workspace.clone()) };
            probe
                .edit(&old_key, |state| {
                    state.draft = "A draft survives failed B refresh".into();
                    state.fail_update = true;
                })
                .unwrap();
            let scope = runtime_scope(h.app.panel_runtime().unwrap());
            h.app
                .panel_runtime_mut()
                .unwrap()
                .with_services(|_, services| {
                    services.local.request(&scope, LocalRequest::KeyboardOwner { instance: id("editor"), control: ControlId::new("draft").unwrap() })
                })
                .unwrap();
            let target = HostControlTarget::Pointer(if captured {
                HostPointerTarget::Header { instance: id("editor") }
            } else {
                HostPointerTarget::Menu { instance: id("editor") }
            });
            let control = app_control(&h.app, target);
            let at = egui_rect(control.bounds).center();
            h.ctx.memory_mut(|memory| memory.request_focus(control.id));
            // Establish transient ownership before enabling the service failure.
            probe.edit(&old_key, |state| state.fail_update = false).unwrap();
            h.frame_with(vec![egui::Event::PointerMoved(at), button_event(at, true)]);
            if captured {
                h.frame_with(vec![egui::Event::PointerMoved(at + egui::vec2(12.0, 0.0))]);
                assert!(h.app.panel_runtime_mut().unwrap().host().unwrap().interaction().is_captured());
            } else {
                h.frame_with(vec![button_event(at, false)]);
                h.frames(2);
                assert!(h.app.universal_panel_frame().unwrap().menu_bounds.is_some());
            }
            probe.edit(&old_key, |state| state.fail_update = true).unwrap();
            let old = probe.state(&old_key).unwrap();
            let workspace = probe.state(&workspace_key).unwrap();
            let layout = h.app.panel_runtime().unwrap().layout().layout().clone();
            let factory_keys = trace.borrow().factory_keys.clone();
            let controllers = h.app.panel_runtime().unwrap().controller_count();
            let incoming = solidcraft_engine::Session::default();
            crate::documents::open_tab(&mut h.app, incoming).unwrap();
            h.app.home.open = false;
            // Switching documents has its own revision invalidation. The
            // refresh/layout boundary below must preserve this switched state.
            let document = h.app.session.doc.clone();
            let model = h.app.session.model.state();
            let revision = h.app.session.revision;
            let undo = h.app.session.undo.len();
            let dirty = h.app.session.is_dirty();
            let incoming_key = app_editor_key(&h.app);
            assert_ne!(incoming_key.owner, old_key.owner);
            trace.borrow_mut().fail_refresh = true;
            trace.borrow_mut().model_reads.clear();
            let time = h.ctx.input(|input| input.time) + 1.0 / 60.0;
            let mut passes = 0;
            let mut output = h.ctx.run_ui(
                egui::RawInput { screen_rect: Some(egui_rect(capacity())), time: Some(time), focused: true, ..Default::default() },
                |ui| {
                    passes += 1;
                    h.app.logic(ui.ctx());
                    h.app.ui(ui);
                    // Repeated same-frame boundaries must fail before admitting B,
                    // and never turn reconciliation into another background tick.
                    let expected = error(Rule::Availability, "boundary refresh refused");
                    assert_eq!(h.app.panel_content_action(&id("editor"), &ControlId::new("apply").unwrap()).unwrap_err(), expected);
                    assert_eq!(h.app.dispatch_panel_layout(transaction(vec![Mutation::Close { instance: id("editor") }])).unwrap_err(), expected);
                    assert_eq!(h.app.panel_runtime.as_mut().unwrap().prepare_host(&h.app.session, capacity()).unwrap_err(), expected);
                    assert_eq!(h.app.mount_panels(&ResolvedLayout::default(), &mut FakePanelHost::default()).unwrap_err(), expected);
                    if ui.ctx().current_pass_index() == 0 {
                        ui.ctx().request_discard("failed refresh still receives one retained-service tick");
                    }
                },
            );
            output.textures_delta.clear();
            assert!(passes >= 2);
            assert_eq!(h.app.panel_runtime.as_ref().unwrap().last_error(), Some(&error(Rule::Availability, "boundary refresh refused")));
            assert_eq!(h.app.panel_runtime().unwrap().environment(), &environment);
            assert_eq!(trace.borrow().document, environment.document);
            assert_eq!(trace.borrow().factory_keys, factory_keys);
            assert_eq!(h.app.panel_runtime().unwrap().controller_count(), controllers);
            assert!(probe.state(&incoming_key).is_err());
            assert!(trace.borrow().model_reads.iter().all(|owner| owner != &incoming_key.owner));
            assert!(trace.borrow().commands.is_empty());
            assert_eq!(h.app.panel_runtime().unwrap().layout().layout(), &layout);
            let errors = h.app.panel_runtime().unwrap().update_errors();
            assert_eq!(errors.len(), 1);
            assert_eq!(errors[0].controller.as_ref(), Some(&old_key));
            assert_eq!(errors[0].error, error(Rule::Lifecycle, "synthetic service update failed"));
            assert!(h.app.universal_panel_frame().is_none());
            assert!(!h.app.panel_runtime_mut().unwrap().host().unwrap().interaction().is_captured());
            assert_ne!(h.ctx.memory(|memory| memory.focused()), Some(control.id));
            assert!(!trace.borrow().keyboard.contains_key(&scope));
            for (key, before) in [(&old_key, &old), (&workspace_key, &workspace)] {
                let after = probe.state(key).unwrap();
                assert_eq!(after.updates, before.updates + 1, "every retained service gets one attempt, even after another fails");
                assert_eq!(after.renders, before.renders);
                assert!(!h.app.panel_runtime().unwrap().state(key).unwrap().presented);
                assert_eq!(
                    (after.cancels, after.applies, after.finishes, after.destroys),
                    (before.cancels, before.applies, before.finishes, before.destroys)
                );
            }
            assert_eq!(probe.state(&old_key).unwrap().draft, old.draft);
            assert!(probe.state(&old_key).unwrap().transaction_live);
            assert_eq!(h.app.session.revision, revision);
            assert_eq!(h.app.session.undo.len(), undo);
            assert_eq!(h.app.session.is_dirty(), dirty);
            assert!(std::sync::Arc::ptr_eq(&document, &h.app.session.doc));
            assert!(std::sync::Arc::ptr_eq(&model, &h.app.session.model.state()));

            trace.borrow_mut().fail_refresh = false;
            probe.edit(&old_key, |state| state.fail_update = false).unwrap();
            let time = time + 1.0 / 60.0;
            let mut output = h.ctx.run_ui(
                egui::RawInput { screen_rect: Some(egui_rect(capacity())), time: Some(time), focused: true, ..Default::default() },
                |ui| {
                    h.app.logic(ui.ctx());
                    h.app.ui(ui);
                    if ui.ctx().current_pass_index() == 0 {
                        ui.ctx().request_discard("recovery mounts must not add a service update");
                    }
                },
            );
            output.textures_delta.clear();
            assert_eq!(trace.borrow().document, Some(DocumentId::new(h.app.session.document_id.clone()).unwrap()));
            assert_eq!(h.app.panel_runtime().unwrap().environment().document, trace.borrow().document);
            assert_eq!(trace.borrow().factory_keys.iter().filter(|key| **key == incoming_key).count(), 1);
            let current = probe.state(&incoming_key).unwrap();
            assert_eq!(current.updates, 1);
            assert!(current.renders > 0);
            assert_eq!(probe.state(&old_key).unwrap().updates, old.updates + 2);
            assert_eq!(probe.state(&old_key).unwrap().renders, old.renders);
            assert_eq!(probe.state(&workspace_key).unwrap().updates, workspace.updates + 2);
            assert!(h.app.panel_runtime().unwrap().update_errors().is_empty());
            h.app.panel_content_action(&id("editor"), &ControlId::new("apply").unwrap()).unwrap();
            assert_eq!(trace.borrow().commands, vec![incoming_key.owner.clone()]);
            assert_eq!(probe.state(&incoming_key).unwrap().applies, 1);
            assert_eq!(probe.state(&incoming_key).unwrap().updates, 1);
            assert_eq!(probe.state(&old_key).unwrap().draft, old.draft);
            assert_eq!(h.app.panel_runtime().unwrap().layout().layout(), &layout);
        }
    }
    // Explicit tick's returned error is examined in a separate fresh app so it
    // cannot accidentally grant the automatic-frame assertion an extra update.
    let (mut h, probe, trace) = boundary_app();
    let key = app_editor_key(&h.app);
    let environment = h.app.panel_runtime().unwrap().environment().clone();
    probe.edit(&key, |state| state.fail_update = true).unwrap();
    crate::documents::open_tab(&mut h.app, solidcraft_engine::Session::default()).unwrap();
    trace.borrow_mut().fail_refresh = true;
    assert_eq!(h.app.tick_panel_runtime().unwrap_err(), error(Rule::Availability, "boundary refresh refused"));
    assert_eq!(h.app.panel_runtime().unwrap().environment(), &environment);
    assert_eq!(h.app.panel_runtime().unwrap().update_errors()[0].controller.as_ref(), Some(&key));
}

#[test]
fn late_first_appearance_window_blocks_sash_double_click_reset() {
    for scale in [1.0, 2.0] {
        for cover_on_press in [false, true] {
            let mut content = host_logs(2, Zone::Bottom);
            content
                .dispatch(vec![Mutation::ResizePair {
                    first: section("section-0"),
                    second: section("section-1"),
                    first_size: Size { width: 360.0, height: 240.0 },
                    second_size: Size { width: 200.0, height: 240.0 },
                }])
                .unwrap();
            let mut raw = RawPanelHost::new(content, scale);
            raw.prime_foreign_window();
            raw.frame(Vec::new(), false);
            raw.frame(Vec::new(), false);
            let target = HostControlTarget::Pointer(HostPointerTarget::Sash { first: section("section-0"), second: section("section-1") });
            raw.click(target.clone(), false);
            let at = raw.at(target.clone());
            let before = raw.content.runtime.layout().inspect();
            let native_commits = raw.content.native.commits;
            let cover = egui::Rect::from_center_size(at, egui::vec2(100.0, 80.0));
            if cover_on_press {
                raw.foreign_cover = Some(cover);
            }
            raw.pointer(at, true, false);
            raw.foreign_cover = Some(cover);
            raw.pointer(at, false, true);
            assert_no_host_mutation(&raw, &before, native_commits);
            raw.foreign_cover = None;
            raw.frame(Vec::new(), true);
            raw.pointer(at, false, true);
            assert_no_host_mutation(&raw, &before, native_commits);
            // Expire the earlier click's time window, then prove real response
            // double-click remains a single ResetPair through release multipass.
            raw.time += 1.0;
            raw.frame(Vec::new(), false);
            raw.click(target.clone(), false);
            raw.click(target, true);
            assert_eq!(raw.content.runtime.layout().inspect().revision, before.revision + 1);
            let commits = raw
                .host
                .frame()
                .unwrap()
                .events
                .iter()
                .filter_map(|event| match event {
                    HostEvent::PreparedCommitted { reply } => Some(reply),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(commits.len(), 1);
            assert_eq!(commits[0].revision, before.revision + 1);
            let sections = &raw.content.runtime.layout().layout().sections;
            assert_eq!((sections[0].size.width, sections[1].size.width), (698.0, 698.0));
            raw.frame(Vec::new(), true);
            assert_eq!(raw.host.frame().unwrap().layout.sections.iter().map(|card| card.outer.size.width).collect::<Vec<_>>(), vec![698.0, 698.0]);
            raw.pointer(at, false, true);
            assert_eq!(raw.content.runtime.layout().inspect().revision, before.revision + 1);
            assert_eq!(raw.content.native.commits, native_commits + 1);
            assert!(raw.content.native.hosts.is_empty());
            assert_eq!(raw.content.commands.calls, 0);
        }
    }
}

#[test]
fn exhausted_single_pass_sizing_and_collapsed_headers_publish_only_actual_footprints() {
    for scale in [1.0, 2.0] {
        let mut content = host_logs(1, Zone::Floating);
        content.dispatch(vec![Mutation::Focus { instance: id("log-0") }]).unwrap();
        let mut raw = RawPanelHost::new(content, scale);
        raw.ctx.options_mut(|options| options.max_passes = 1.try_into().unwrap());
        let sizing = raw.frame(Vec::new(), false).0;
        assert_eq!(raw.painted_frames.len(), 1);
        let card = sizing.layout.sections.iter().find(|card| card.instance == id("log-0")).unwrap();
        assert_eq!(sizing.layout.resolved.panels.len(), 1, "intended geometry survives invisible measurement");
        for regions in [&sizing.layout.painted, &sizing.layout.input, &sizing.layout.occlusion, &sizing.layout.reservations] {
            assert!(!regions.iter().any(|bounds| egui_rect(*bounds).intersect(egui_rect(card.outer)).is_positive()));
        }
        assert!(sizing.controls.is_empty(), "no metadata-only controls from an invisible Area");
        let key = ControllerKey { instance: id("log-0"), owner: raw.content.runtime.instance_owner(&id("log-0")).unwrap() };
        assert_eq!(raw.content.probe.state(&key).unwrap().renders, 0, "measurement must not render hidden content");
        let visible = raw.frame(Vec::new(), false).0;
        assert!(visible.layout.painted.contains(&card.outer));
        assert!(visible.layout.input.contains(&card.outer));
        assert!(visible.layout.occlusion.contains(&card.outer));
        let focused = raw.ctx.memory(|memory| memory.focused()).unwrap();
        assert!(visible.controls.iter().any(|control| control.id == focused && control.enabled));
        assert!(raw.content.probe.state(&key).unwrap().renders > 0);

        let mut side = RawPanelHost::new(host_logs(1, Zone::Left), scale);
        side.ctx.options_mut(|options| options.max_passes = 1.try_into().unwrap());
        side.frame(Vec::new(), false);
        let focus = side.content.dispatch(vec![Mutation::Focus { instance: id("log-0") }]).unwrap();
        side.host.note_reply(&focus);
        side.content.dispatch(vec![Mutation::Collapse { section: section("section-0"), collapsed: true }]).unwrap();
        let collapsed = side.frame(Vec::new(), false).0;
        let header = collapsed.layout.sections.first().unwrap();
        assert!(header.collapsed && header.body.is_none() && header.footer.is_none());
        assert_eq!(header.outer, header.header);
        assert_eq!(collapsed.layout.painted, vec![header.outer]);
        assert_eq!(collapsed.layout.input, vec![header.outer]);
        assert!(collapsed.controls.iter().any(|control| matches!(&control.target,
            HostControlTarget::Pointer(HostPointerTarget::Collapse { instance }) if *instance == id("log-0"))));
        let hidden_renders = side
            .content
            .probe
            .state(&ControllerKey { instance: id("log-0"), owner: side.content.runtime.instance_owner(&id("log-0")).unwrap() })
            .unwrap()
            .renders;
        side.frame(Vec::new(), false);
        assert_eq!(
            side.content
                .probe
                .state(&ControllerKey { instance: id("log-0"), owner: side.content.runtime.instance_owner(&id("log-0")).unwrap() })
                .unwrap()
                .renders,
            hidden_renders
        );

        let mut popup = RawPanelHost::new(host_logs(1, Zone::Left), scale);
        popup.ctx.options_mut(|options| options.max_passes = 1.try_into().unwrap());
        popup.frame(Vec::new(), false);
        popup.frame(Vec::new(), false);
        popup.click(HostControlTarget::Pointer(HostPointerTarget::Menu { instance: id("log-0") }), false);
        assert_no_popup(popup.host.frame().unwrap());
        popup.frame(Vec::new(), false);
        assert_no_popup(popup.host.frame().unwrap());
        assert!(popup.host.frame().unwrap().owned_overlays.is_empty());
        popup.frame(Vec::new(), false);
        assert!(popup.host.frame().unwrap().menu_bounds.is_some());
        assert!(popup.host.frame().unwrap().controls.iter().any(|control| matches!(control.target, HostControlTarget::Shared { .. })));
    }
}
#[test]
fn explicit_late_foreign_raise_blocks_popup_actions_but_lower_and_background_layers_do_not() {
    fn assert_selected_panel_menu(raw: &mut RawPanelHost) {
        assert!(
            raw.host.frame().unwrap().controls.iter().any(|control| {
                control.target == HostControlTarget::Shared { instance: id("log-0"), action: HostSharedAction::Move(Zone::Right) }
            })
        );
        assert!(!raw.host.frame().unwrap().controls.iter().any(|control| control.target == HostControlTarget::PanelMenu { instance: id("log-0") }));
        let inactive_identity =
            |control: &HostFrameControl| matches!(&control.target, HostControlTarget::PanelMenu { instance } if *instance != id("log-0"));
        let menu_at = egui_rect(raw.host.frame().unwrap().menu_bounds.unwrap()).center();
        for _ in 0..8 {
            if raw.host.frame().unwrap().controls.iter().any(inactive_identity) {
                break;
            }
            raw.frame(review_wheel(menu_at, -120.0), false);
        }
        assert!(raw.host.frame().unwrap().controls.iter().any(inactive_identity), "inactive tab identities remain reachable by scrolling");
    }
    for scale in [1.0, 2.0] {
        for nested in [false, true] {
            for order in [egui::Order::Foreground, egui::Order::Background] {
                for raise in [false, true] {
                    let (mut raw, source, button) = raw_popup_fixture(scale, if nested { PopupFixture::Tabs } else { PopupFixture::Menu });
                    raw.foreign_order = order;
                    raw.foreign_cover = Some(egui::Rect::from_min_size(egui::pos2(900.0, 700.0), egui::vec2(100.0, 80.0)));
                    raw.frame(Vec::new(), false);
                    raw.frame(Vec::new(), false);
                    // Keep the sized foreign Area continuously visible while
                    // the newer popup establishes its higher stack position.
                    open_root_popup(&mut raw, &source, button, false);
                    let target = if nested {
                        HostControlTarget::PanelMenu { instance: id("log-0") }
                    } else {
                        HostControlTarget::Shared { instance: id("log-0"), action: HostSharedAction::Move(Zone::Right) }
                    };
                    let at = raw.at(target.clone());
                    raw.foreign_cover = Some(egui::Rect::from_center_size(at, egui::vec2(100.0, 40.0)));
                    raw.frame(vec![egui::Event::PointerMoved(at)], false);
                    let source_widget = raw_control(&raw, &target);
                    let source_layer = raw.ctx.read_response(source_widget.id).unwrap().layer_id;
                    assert_eq!(raw.ctx.layer_id_at(at), Some(source_layer), "an unraised lower foreign Area cannot block the popup");
                    assert!(raw.foreign_responses.iter().all(egui::Response::enabled));
                    let before = raw.content.runtime.layout().inspect();
                    let bounds = raw.host.frame().unwrap().menu_bounds;
                    let commits = raw.content.native.commits;
                    raw.pointer(at, true, false);
                    assert!(raw.ctx.read_response(source_widget.id).unwrap().is_pointer_button_down_on());
                    raw.foreign_raise_late = raise;
                    raw.pointer(at, false, true);
                    let blocked = raise && order == egui::Order::Foreground;
                    if blocked {
                        assert_no_host_mutation(&raw, &before, commits);
                        assert_eq!(raw.host.frame().unwrap().menu_bounds, bounds);
                    } else if nested {
                        assert_eq!(raw.content.runtime.layout().inspect(), before);
                    } else {
                        assert_eq!(raw.content.runtime.layout().inspect().revision, before.revision + 1);
                        assert_eq!(raw.content.runtime.layout().layout().containing(&id("log-0")).unwrap().location.zone(), Zone::Right);
                        assert_eq!(raw.content.native.commits, commits + 1);
                    }
                    raw.foreign_raise_late = false;
                    raw.foreign_cover = None;
                    raw.frame(Vec::new(), true);
                    raw.pointer(at, false, true);
                    if blocked {
                        assert_no_host_mutation(&raw, &before, commits);
                        assert_eq!(raw.host.frame().unwrap().menu_bounds, bounds);
                        assert!(raw.host.frame().unwrap().controls.iter().any(|control| control.target == target));
                    } else if nested {
                        assert_selected_panel_menu(&mut raw);
                    } else {
                        assert_eq!(raw.content.runtime.layout().inspect().revision, before.revision + 1);
                    }
                    assert_eq!(raw.content.commands.calls, 0);
                    assert!(raw.content.native.hosts.is_empty());
                }
            }
        }
    }
    // A third, now-undrawn Area can mask the stored order of the two live
    // layers in layer_id_at. Compare their real stored ranks, independently of
    // whichever stale third layer happens to be returned at the pointer.
    for scale in [1.0, 2.0] {
        for raise in [false, true] {
            let (mut raw, source, button) = raw_popup_fixture(scale, PopupFixture::Tabs);
            raw.ctx.global_style_mut(|style| style.interaction.interact_radius = 6.0);
            raw.foreign_cover = Some(egui::Rect::from_min_size(egui::pos2(900.0, 700.0), egui::vec2(100.0, 80.0)));
            raw.frame(Vec::new(), false);
            raw.frame(Vec::new(), false);
            open_root_popup(&mut raw, &source, button, false);
            let target = HostControlTarget::PanelMenu { instance: id("log-0") };
            let at = raw.at(target.clone());
            raw.foreign_cover = Some(egui::Rect::from_center_size(at, egui::vec2(100.0, 40.0)));
            raw.frame(vec![egui::Event::PointerMoved(at)], false);
            let widget = raw_control(&raw, &target);
            let source_layer = raw.ctx.read_response(widget.id).unwrap().layer_id;
            assert_eq!(raw.ctx.layer_id_at(at), Some(source_layer));
            // This real tiny hover-only Area contributes layer geometry while
            // leaving the nearby source button eligible for egui's six-point
            // interaction radius. The click proof below guards that premise.
            raw.stale_foreign_cover = Some(egui::Rect::from_center_size(at, egui::vec2(1.0, 1.0)));
            raw.frame(Vec::new(), false);
            raw.frame(Vec::new(), false);
            let stale = raw.stale_foreign_layer.unwrap();
            assert_ne!(stale, source_layer);
            assert_eq!(raw.ctx.layer_id_at(at), Some(stale));
            raw.stale_foreign_cover = None;
            raw.foreign_raise_late = raise;
            let before = raw.content.runtime.layout().inspect();
            let bounds = raw.host.frame().unwrap().menu_bounds;
            let commits = raw.content.native.commits;
            raw.frame(vec![egui::Event::PointerMoved(at), button_event(at, true), button_event(at, false)], false);
            assert!(raw.clicked_widgets.contains(&widget.id), "the real popup response must offer an action before deferred layer validation");
            raw.foreign_raise_late = false;
            raw.foreign_cover = None;
            raw.frame(Vec::new(), true);
            if raise {
                assert_no_host_mutation(&raw, &before, commits);
                assert_eq!(raw.host.frame().unwrap().menu_bounds, bounds);
                assert!(raw.host.frame().unwrap().controls.iter().any(|control| control.target == target));
            } else {
                assert_eq!(raw.content.runtime.layout().inspect(), before);
                assert_selected_panel_menu(&mut raw);
            }
        }
    }
}

#[test]
fn native_reply_capabilities_match_live_sink_across_prepare_show_finish_stale_and_error_paths() {
    for layout_native in [false, true] {
        for adapter_native in [false, true] {
            let mut content = PanelMountHarness::new(layout_native).unwrap();
            open_editor(&mut content);
            content.native = FakeNativeAdapter::new(adapter_native);
            if adapter_native {
                content.native.capabilities.max_native_hosts = 3;
            }
            let authority = content.runtime.layout().inspect().capabilities;
            let native = content.native.capabilities();
            let expected = HostCapabilities {
                native_windows: authority.native_windows && native.native_windows,
                persistence: authority.persistence,
                max_native_hosts: authority.max_native_hosts.min(native.max_native_hosts),
            };
            let mut raw = RawPanelHost::new(content, 1.0);
            raw.host.capture_metrics(&raw.content.runtime, &raw.content.models).unwrap();
            raw.host.prepare(&raw.content.runtime, raw.capacity).unwrap();
            assert!(!raw.host.frame().unwrap().reply.capabilities.native_windows, "standalone first prepare has no native owner yet");
            raw.frame(Vec::new(), true);
            for frame in raw.painted_frames.iter().chain(&raw.finished_frames) {
                assert_eq!(frame.reply.capabilities, expected);
            }
            raw.host.capture_metrics(&raw.content.runtime, &raw.content.models).unwrap();
            raw.host.prepare(&raw.content.runtime, raw.capacity).unwrap();
            assert_eq!(raw.host.frame().unwrap().reply.capabilities, expected, "a capable first show must recover from prepare's conservative false");
            let inspected = raw.content.command("layout.get", &serde_json::json!({})).unwrap();
            assert_eq!(inspected["state"]["capabilities"], serde_json::to_value(&expected).unwrap());
            let sink_reply = {
                let h = &mut raw.content;
                RuntimeActionSink {
                    runtime: &mut h.runtime,
                    native: &mut h.native,
                    services: &mut ContentServices { models: &h.models, commands: &mut h.commands, local: &mut h.local },
                }
                .inspect()
            };
            assert_eq!(sink_reply.capabilities, expected);
            raw.frame_late(Vec::new(), true, |h, ctx| {
                if ctx.current_pass_index() == 0 {
                    h.dispatch(vec![Mutation::Collapse { section: section("editor-section"), collapsed: true }]).unwrap();
                }
            });
            for frame in raw.painted_frames.iter().chain(&raw.finished_frames) {
                assert_eq!(frame.reply.capabilities, expected);
            }
            assert!(
                raw.finished_frames[0].events.iter().any(|event| matches!(event, HostEvent::Rejected { error, .. } if error.rule == Rule::Revision))
            );
            raw.content.dispatch(vec![Mutation::Collapse { section: section("editor-section"), collapsed: false }]).unwrap();
            raw.frame(Vec::new(), false);
            raw.frame(Vec::new(), false);
            raw.menu(id("editor"));
            let popout = HostControlTarget::Shared { instance: id("editor"), action: HostSharedAction::Popout };
            if !expected.native_windows {
                assert!(raw.host.frame().unwrap().controls.iter().all(|control| control.target != popout));
                continue;
            }
            assert!(raw_control(&raw, &popout).enabled);
            raw.content.native.fail_prepare = true;
            let before = raw.content.runtime.layout().inspect();
            raw.click(popout, true);
            for frame in raw.painted_frames.iter().chain(&raw.finished_frames) {
                assert_eq!(frame.reply.capabilities, expected);
            }
            assert_eq!(raw.content.runtime.layout().inspect(), before);
            if expected.native_windows {
                assert!(
                    raw.host
                        .frame()
                        .unwrap()
                        .events
                        .iter()
                        .any(|event| matches!(event, HostEvent::Rejected { error, .. } if error.rule == Rule::Capability))
                );
            }
        }
    }
    // App preparation already knows its persistent native owner before painting.
    for native in [false, true] {
        let (mut h, _, _) = boundary_app();
        let adapter = std::rc::Rc::new(std::cell::RefCell::new(FakeNativeAdapter::new(native)));
        h.app.set_panel_native_adapter(Box::new(SharedNative(adapter))).unwrap();
        h.app.panel_runtime.as_mut().unwrap().prepare_host(&h.app.session, capacity()).unwrap();
        let command = h.app.run_panel_command("layout.get", &serde_json::json!({})).unwrap();
        assert_eq!(serde_json::to_value(&h.app.universal_panel_frame().unwrap().reply.capabilities).unwrap(), command["state"]["capabilities"]);
        assert_eq!(h.app.universal_panel_frame().unwrap().reply.capabilities.native_windows, native);
    }
}

// Review integration fixtures retain diagnostic identity through preparation.
// These cases are source-only until root runs the complete integration gate.
#[derive(Default)]
struct ReviewCohortTrace {
    factories: Vec<(ControllerKey, usize)>,
    measures: Vec<usize>,
    providers: Vec<(usize, std::sync::Arc<dyn HostContentMetrics>)>,
    lifecycle: Vec<(usize, LifecycleEvent)>,
    actions: Vec<(ControllerKey, usize)>,
    unavailable: std::collections::BTreeSet<ControllerKey>,
    availability_reads: usize,
    disabled: bool,
    metric_revision: u64,
    editors: Vec<(egui::Id, ContentSlot, egui::Rect, String)>,
    wrap_rows: Vec<usize>,
    consume_wheel: bool,
    omit_editor: bool,
    child_scroll: std::collections::BTreeMap<InstanceId, (egui::Rect, f32)>,
}
type ReviewTrace = std::rc::Rc<std::cell::RefCell<ReviewCohortTrace>>;
#[derive(Debug)]
struct ReviewMetrics {
    revision: u64,
    height: f32,
    policy: SideHeightPolicy,
}
impl HostContentMetrics for ReviewMetrics {
    fn revision(&self) -> u64 {
        self.revision
    }
    fn measure(&self, request: &ContentMeasureRequest) -> PanelResult<SideContentMeasure> {
        Ok(SideContentMeasure { request: request.clone(), needed_height: self.height.max(request.chrome_height), policy: self.policy })
    }
}
struct ReviewController {
    inner: Box<dyn PanelController>,
    key: ControllerKey,
    serial: usize,
    trace: ReviewTrace,
    draft: String,
}
impl PanelController for ReviewController {
    fn availability(&self, owner: &ContentOwner, models: &dyn PanelModels) -> Availability {
        let unavailable = {
            let mut trace = self.trace.borrow_mut();
            trace.availability_reads += 1;
            trace.unavailable.contains(&self.key)
        };
        if unavailable { Availability::Unavailable { reason: "review owner unavailable".into() } } else { self.inner.availability(owner, models) }
    }
    fn measure(&self, _owner: &ContentOwner, _models: &dyn PanelModels, spec: &PanelSpec) -> PanelResult<std::sync::Arc<dyn HostContentMetrics>> {
        let mut trace = self.trace.borrow_mut();
        trace.measures.push(self.serial);
        let wrapping = self.key.instance.0 == "wrapping-editor";
        let characters = if wrapping { self.draft.chars().count() } else { 0 };
        // Explicit bounded preferred height, not a product intrinsic measurer.
        let provider: std::sync::Arc<dyn HostContentMetrics> = std::sync::Arc::new(ReviewMetrics {
            revision: trace.metric_revision + self.serial as u64 * 1024 + ((characters as u64) << 32),
            height: 240.0 + self.serial as f32 + characters as f32,
            policy: spec.side_height_policy,
        });
        trace.providers.push((self.serial, provider.clone()));
        Ok(provider)
    }
    fn measurement_revision(&self, _owner: &ContentOwner, _models: &dyn PanelModels, _spec: &PanelSpec) -> PanelResult<u64> {
        let characters = if self.key.instance.0 == "wrapping-editor" { self.draft.chars().count() } else { 0 };
        Ok(self.trace.borrow().metric_revision + self.serial as u64 * 1024 + ((characters as u64) << 32))
    }
    fn update(&mut self, owner: &ContentOwner, services: &mut ContentServices<'_>) -> PanelResult<()> {
        self.inner.update(owner, services)
    }
    fn presentation(&self) -> PanelPresentation {
        let mut presentation = self.inner.presentation();
        presentation.actions.push(RuntimeAction {
            pressed: Some(!self.trace.borrow().disabled),
            id: ControlId::new("pulse").unwrap(),
            label: "review pulse".into(),
            enabled: !self.trace.borrow().disabled,
            disabled_reason: self.trace.borrow().disabled.then(|| "review binding disabled".into()),
        });
        presentation
    }
    fn prepare_lifecycle(&self, event: &LifecycleEvent) -> PanelResult<()> {
        self.inner.prepare_lifecycle(event)
    }
    fn lifecycle(&mut self, event: &LifecycleEvent) {
        self.trace.borrow_mut().lifecycle.push((self.serial, event.clone()));
        self.inner.lifecycle(event);
    }
    fn render(&mut self, slot: ContentSlot, ui: &mut egui::Ui, context: &PanelContext, services: &mut ContentServices<'_>) -> PanelResult<()> {
        self.inner.render(slot, ui, context, services)?;
        let consume_wheel = self.trace.borrow().consume_wheel;
        if slot == ContentSlot::Body {
            if self.trace.borrow().omit_editor {
                return Ok(());
            }
            if self.key.instance.0 == "inline-editor" || self.key.instance.0 == "wrapping-editor" {
                let editor_id = egui::Id::unique(("review-inline-editor", &self.key));
                let response = if self.key.instance.0 == "wrapping-editor" {
                    crate::ui_kit::text_area(ui, "Review wrapping editor", editor_id, &mut self.draft, 1, 1024)
                } else {
                    crate::ui_kit::bounded_cell_editor(ui, editor_id, &mut self.draft, "Review editable value", false, 4096)
                };
                if self.key.instance.0 == "wrapping-editor" {
                    let tokens = solidcraft_theme::Tokens::get();
                    let width = (response.rect.width() - 2.0 * tokens.form_inset).max(1.0);
                    let galley =
                        ui.fonts_mut(|fonts| fonts.layout(self.draft.clone(), egui::FontId::monospace(tokens.font_label), tokens.form_ink, width));
                    self.trace.borrow_mut().wrap_rows.push(galley.rows.len());
                }
                services.local.register_focus_response(
                    &LocalScope { workspace: context.workspace.clone(), document: context.document.clone(), viewport: context.viewport.clone() },
                    &context.instance,
                    &response,
                )?;
                self.trace.borrow_mut().editors.push((response.id, slot, response.rect, self.draft.clone()));
            } else if consume_wheel {
                let child = egui::ScrollArea::vertical()
                    .id_salt(("review-child-scroll", &self.key))
                    .auto_shrink([false, false])
                    .max_height(ui.available_height().max(1.0))
                    .show(ui, |ui| {
                        ui.label("Real child scroll content");
                        ui.add_space(1200.0);
                    });
                self.trace
                    .borrow_mut()
                    .child_scroll
                    .insert(self.key.instance.clone(), (child.inner_rect.intersect(ui.clip_rect()), child.state.unclamped_offset().y));
            }
        }
        Ok(())
    }
    fn action(&mut self, owner: &ContentOwner, action: &ControlId, services: &mut ContentServices<'_>) -> PanelResult<()> {
        if action.0 == "pulse" {
            self.trace.borrow_mut().actions.push((self.key.clone(), self.serial));
            Ok(())
        } else {
            self.inner.action(owner, action, services)
        }
    }
}
fn review_cohort_host(edit: impl Fn(&mut PanelSpec)) -> (PanelMountHarness, ReviewTrace) {
    review_cohort_host_with_layout(edit, |_| {})
}
fn review_cohort_host_with_layout(edit: impl Fn(&mut PanelSpec), setup: impl FnOnce(&mut LayoutService)) -> (PanelMountHarness, ReviewTrace) {
    let mut h = PanelMountHarness::without_content(false).unwrap();
    let environment = h.runtime.environment().clone();
    let mut registry = PanelRegistry::default();
    for original in synthetic_registry().unwrap().specs() {
        let mut spec = original.clone();
        spec.header_actions.push(HeaderActionSpec { id: ControlId::new("pulse").unwrap(), label_key: "ui.panel".into(), icon: "list".into() });
        spec.features.hot_closable = true;
        edit(&mut spec);
        registry.register(spec).unwrap();
    }
    let mut service = LayoutService::new(registry, Layout::empty(environment.workspace.clone()), HostCapabilities::default()).unwrap();
    setup(&mut service);
    h.runtime = PanelRuntime::new(service, environment).unwrap();
    let trace = ReviewTrace::default();
    for (kind, editor, ownership) in [("fixture.log", false, Ownership::Workspace), ("fixture.editor", true, Ownership::Document)] {
        let binding = fixture_controller_binding(&h.probe, editor, ownership);
        let create = binding.create;
        let trace = trace.clone();
        h.runtime
            .register(
                PanelKind::new(kind).unwrap(),
                ControllerBinding {
                    ownership,
                    create: Box::new(move |instance, owner| {
                        let key = ControllerKey { instance: instance.id.clone(), owner: owner.clone() };
                        let serial = trace.borrow().factories.len() + 1;
                        let inner = create(instance, owner)?;
                        trace.borrow_mut().factories.push((key.clone(), serial));
                        let draft = if key.instance.0 == "wrapping-editor" { "short" } else { "retained editable value" }.into();
                        Ok(Box::new(ReviewController { inner, key, serial, trace: trace.clone(), draft }))
                    }),
                },
            )
            .unwrap();
    }
    (h, trace)
}
fn review_header_action(instance: &str) -> LayoutAction {
    transaction(vec![Mutation::HeaderAction { instance: id(instance), action: ControlId::new("pulse").unwrap() }])
}

#[test]
fn captured_incoming_workspace_cohort_is_created_measured_and_committed_exactly_once() {
    let (mut h, trace) = review_cohort_host(|_| {});
    h.dispatch(vec![fixture_open("fixture.log", "log", "log", Zone::Left, 0).unwrap()]).unwrap();
    h.tick().unwrap();
    let mut host = WorkspacePanelHost::new(HostOptions::default()).unwrap();
    host.capture_metrics(&h.runtime, &h.models).unwrap();
    host.capture_reservations(&h.runtime, &h.catalogue).unwrap();
    host.prepare(&h.runtime, capacity()).unwrap();
    let mut incoming = h.runtime.layout().layout().clone();
    incoming.workspace = WorkspaceId::new("review.incoming").unwrap();
    let action = LayoutAction::Replace { expected_revision: Some(h.runtime.layout().inspect().revision), layout: incoming };
    let count = trace.borrow().factories.len();
    let measures = trace.borrow().measures.len();
    let mut services = ContentServices { models: &h.models, commands: &mut h.commands, local: &mut h.local };
    {
        let mut live = RuntimeActionSink { runtime: &mut h.runtime, native: &mut h.native, services: &mut services };
        let mut sink = super::app::AdmittedActionSink { live: &mut live, host: Some(&mut host), proposal: std::cell::RefCell::new(None) };
        let first = sink.project(action.clone()).unwrap();
        assert_eq!(sink.project(action.clone()).unwrap(), first);
        assert_eq!(trace.borrow().factories.len(), count + 1);
        assert_eq!(trace.borrow().measures.len(), measures + 1);
        sink.dispatch(action).unwrap();
    }
    let serial = trace.borrow().factories.last().unwrap().1;
    assert_eq!(trace.borrow().factories.len(), count + 1);
    assert_eq!(trace.borrow().measures.len(), measures + 1);
    assert!(trace.borrow().lifecycle.iter().any(|(id, event)| *id == serial && matches!(event, LifecycleEvent::State(_))));
    assert_eq!(h.runtime.controller_count(), 2, "outgoing and incoming workspace controllers remain retained");
    assert_eq!(h.runtime.environment().workspace.0, "review.incoming");
}

#[test]
fn proposed_unavailable_and_unbound_owners_remain_requested_open_without_false_bodies() {
    let (mut h, trace) = review_cohort_host(|_| {});
    let mut incoming_service = fixture_service(false).unwrap();
    apply_fixture(
        &mut incoming_service,
        vec![
            fixture_open("fixture.log", "log", "log", Zone::Left, 0).unwrap(),
            fixture_open("fixture.application", "unbound", "unbound", Zone::Right, 1).unwrap(),
        ],
    );
    h.apply_layout(incoming_service.layout().clone(), None).unwrap();
    h.tick().unwrap();
    let mut host = WorkspacePanelHost::new(HostOptions::default()).unwrap();
    host.capture_metrics(&h.runtime, &h.models).unwrap();
    host.capture_reservations(&h.runtime, &h.catalogue).unwrap();
    host.prepare(&h.runtime, capacity()).unwrap();
    let mut incoming = h.runtime.layout().layout().clone();
    incoming.workspace = WorkspaceId::new("review.unavailable").unwrap();
    trace.borrow_mut().unavailable.insert(ControllerKey { instance: id("log"), owner: ContentOwner::Workspace(incoming.workspace.clone()) });
    let proposal = h
        .runtime
        .prepare_host_proposal(
            host.command_reference(&h.runtime).unwrap(),
            LayoutAction::Replace { expected_revision: None, layout: incoming },
            &h.models,
        )
        .unwrap();
    h.runtime
        .commit_host_proposal(proposal, &mut h.native, &mut ContentServices { models: &h.models, commands: &mut h.commands, local: &mut h.local })
        .unwrap();
    assert!(h.runtime.layout().layout().instances.iter().all(|instance| instance.requested_open));
    assert!(h.runtime.layout().layout().instances.iter().all(|instance| h.runtime.layout().availability(&instance.id) != Availability::Available));
    host.capture_metrics(&h.runtime, &h.models).unwrap();
    host.capture_reservations(&h.runtime, &h.catalogue).unwrap();
    host.prepare(&h.runtime, capacity()).unwrap();
    assert!(host.frame().unwrap().layout.resolved.panels.is_empty());
}

#[test]
fn raw_available_fallback_mounts_and_owns_actions_without_rewriting_remembered_active() {
    for scale in [1.0, 2.0] {
        let (mut h, trace) = review_cohort_host(|_| {});
        h.dispatch(vec![
            fixture_open("fixture.log", "a", "group", Zone::Left, 0).unwrap(),
            Mutation::Open {
                kind: PanelKind::new("fixture.log").unwrap(),
                instance: id("b"),
                destination: Some(Destination::JoinTab { section: section("group"), index: 1 }),
            },
            Mutation::Activate { instance: id("a") },
        ])
        .unwrap();
        let key = ControllerKey { instance: id("a"), owner: ContentOwner::Workspace(h.runtime.environment().workspace.clone()) };
        trace.borrow_mut().unavailable.insert(key.clone());
        let mut raw = RawPanelHost::new(h, scale);
        raw.frame(Vec::new(), false);
        raw.frame(Vec::new(), false);
        assert_eq!(raw.host.frame().unwrap().layout.sections[0].instance, id("b"));
        let b = ControllerKey { instance: id("b"), owner: key.owner.clone() };
        assert!(raw.content.runtime.state(&b).unwrap().activated);
        raw.click(HostControlTarget::Pointer(HostPointerTarget::HeaderAction { instance: id("b"), action: ControlId::new("pulse").unwrap() }), true);
        assert_eq!(trace.borrow().actions.len(), 1);
        assert_eq!(trace.borrow().actions[0].0, b);
        assert_eq!(raw.content.runtime.layout().layout().section(&section("group")).unwrap().active, id("a"));
        assert_eq!(
            raw.content
                .dispatch(match review_header_action("a") {
                    LayoutAction::Apply { mutations, .. } => mutations,
                    other => panic!("header action fixture must use Apply: {other:?}"),
                })
                .unwrap_err()
                .rule,
            Rule::Availability
        );
        trace.borrow_mut().unavailable.remove(&key);
        raw.frame(Vec::new(), false);
        assert_eq!(raw.host.frame().unwrap().layout.sections[0].instance, id("a"));
        assert_eq!(raw.content.runtime.controller_count(), 2);
    }
}

#[test]
fn raw_widget_and_pointer_storage_refusal_preserve_layout_controllers_drafts_and_native_owner() {
    for pointer in [false, true] {
        let mut h = host_logs(2, Zone::Left);
        open_editor(&mut h);
        let key = editor_key(&h);
        h.probe.edit(&key, |state| state.draft = "storage refusal retains this draft".into()).unwrap();
        let mut raw = RawPanelHost::new(h, 1.0);
        raw.frame(Vec::new(), false);
        raw.frame(Vec::new(), false);
        let before = raw.content.runtime.layout().inspect();
        let count = raw.content.runtime.controller_count();
        let commits = raw.content.native.commits;
        raw.content.catalogue.fail_stage = true;
        if pointer {
            let target = raw.host.frame().unwrap().layout.sashes[0].clone();
            let start = raw.at(HostControlTarget::Pointer(HostPointerTarget::Sash { first: target.first, second: target.second }));
            raw.pointer(start, true, false);
            raw.frame(vec![egui::Event::PointerMoved(start + egui::vec2(0.0, 24.0))], false);
            raw.pointer(start + egui::vec2(0.0, 24.0), false, false);
        } else {
            raw.click(HostControlTarget::Pointer(HostPointerTarget::Collapse { instance: id("log-0") }), false);
        }
        assert_host_mutation_refused(&raw, &before, count, commits);
        assert_eq!(raw.content.probe.state(&key).unwrap().draft, "storage refusal retains this draft");
        assert!(
            raw.host.frame().unwrap().events.iter().any(|event| matches!(event, HostEvent::Rejected { error, .. } if error.rule == Rule::Capability))
        );
    }
}
fn assert_host_mutation_refused(raw: &RawPanelHost, before: &LayoutReply, controllers: usize, native_commits: u64) {
    assert_eq!(&raw.content.runtime.layout().inspect(), before);
    assert_eq!(raw.content.runtime.controller_count(), controllers);
    assert_eq!(raw.content.native.commits, native_commits);
}

#[test]
fn raw_registered_header_action_ignores_overflow_storage_but_honours_binding_and_close_permission() {
    for scale in [1.0, 2.0] {
        let (mut h, trace) = review_cohort_host(|spec| {
            spec.features.closable = false;
            spec.features.hot_closable = true;
        });
        h.dispatch(vec![fixture_open("fixture.log", "log", "log", Zone::Left, 0).unwrap()]).unwrap();
        let mut raw = RawPanelHost::new(h, scale);
        raw.capacity.size = Size { width: 400.0, height: 130.0 };
        raw.frame(Vec::new(), false);
        raw.frame(Vec::new(), false);
        let pulse = HostControlTarget::Pointer(HostPointerTarget::HeaderAction { instance: id("log"), action: ControlId::new("pulse").unwrap() });
        let close = HostControlTarget::Pointer(HostPointerTarget::Close { instance: id("log") });
        assert!(!raw_control(&raw, &close).enabled, "hot close visibility does not grant close permission");
        raw.content.catalogue.fail_stage = true;
        raw.click(pulse.clone(), false);
        assert_eq!(trace.borrow().actions.len(), 1);
        trace.borrow_mut().disabled = true;
        raw.frame(Vec::new(), false);
        assert!(!raw_control(&raw, &pulse).enabled);
        raw.click(pulse, false);
        assert_eq!(trace.borrow().actions.len(), 1);
        raw.click(close, false);
        assert!(raw.content.runtime.layout().layout().instance(&id("log")).unwrap().requested_open);
    }
}

#[test]
fn deferred_widget_metric_change_discards_old_geometry_and_requires_fresh_paint() {
    let (mut h, trace) = review_cohort_host(|_| {});
    h.dispatch(vec![fixture_open("fixture.log", "log", "log", Zone::Left, 0).unwrap()]).unwrap();
    let mut raw = RawPanelHost::new(h, 1.0);
    raw.frame(Vec::new(), false);
    raw.frame(Vec::new(), false);
    let target = HostControlTarget::Pointer(HostPointerTarget::Collapse { instance: id("log") });
    let at = raw.at(target);
    raw.pointer(at, true, false);
    let before = raw.content.runtime.layout().inspect();
    let commits = raw.content.native.commits;
    let trace_after = trace.clone();
    // Change immutable snapshot source after paint and before late dispatch.
    let (frame, _) = raw.frame_late(vec![button_event(at, false)], false, move |_, _| {
        trace_after.borrow_mut().metric_revision += 1;
    });
    assert!(frame.is_none(), "a changed snapshot invalidates the old painted frame");
    assert!(!raw.host.interaction().is_captured(), "discarded geometry cannot retain the provisional release candidate");
    assert_eq!(raw.content.runtime.layout().inspect(), before);
    let fresh = raw.frame(Vec::new(), false).0;
    assert!(fresh.preview.is_none());
    assert!(!fresh.events.iter().any(|event| matches!(event, HostEvent::Committed { .. } | HostEvent::PreparedCommitted { .. })));
    assert_eq!(raw.content.native.commits, commits);
    assert!(!raw.content.runtime.layout().layout().section(&section("log")).unwrap().collapsed);
}

#[test]
fn rejected_batch_cancels_capture_before_parse_without_refreshing_owners_or_context() {
    let (mut h, probe, trace) = boundary_app();
    let header = app_control(&h.app, HostControlTarget::Pointer(HostPointerTarget::Header { instance: id("editor") }));
    let at = egui_rect(header.bounds).center();
    boundary_frame(&mut h, vec![egui::Event::PointerMoved(at), button_event(at, true)], |_| {});
    assert!(h.app.panel_runtime.as_ref().unwrap().host().unwrap().interaction().is_captured());
    let before = h.app.panel_runtime().unwrap().layout().inspect();
    let refreshes = trace.borrow().refreshes;
    let factories = trace.borrow().factory_keys.clone();
    let key = app_editor_key(&h.app);
    let draft = probe.state(&key).unwrap().draft;
    let refusal =
        h.app.run_panel_batch(&[serde_json::json!({"command":"panel.close","params":{"target":{"type":"instance","id":"editor"},"unknown":true}})]);
    assert!(refusal.is_err());
    assert!(!h.app.panel_runtime.as_ref().unwrap().host().unwrap().interaction().is_captured());
    assert_eq!(h.app.panel_runtime().unwrap().layout().inspect(), before);
    assert_eq!(trace.borrow().refreshes, refreshes);
    assert_eq!(trace.borrow().factory_keys, factories);
    assert_eq!(probe.state(&key).unwrap().draft, draft);
}

#[test]
fn raw_allocated_pair_endpoints_and_capped_third_have_exact_numeric_deltas() {
    for scale in [1.0, 2.0] {
        for direction in [Direction::Ltr, Direction::Rtl] {
            for bottom in [false, true] {
                for endpoint in [120.0, 676.0] {
                    let zone = if bottom { Zone::Bottom } else { Zone::Left };
                    let mut h = host_with_specs(|spec| {
                        spec.min_size = Size { width: 120.0, height: 120.0 };
                        spec.max_size = Size { width: 2000.0, height: 2000.0 };
                    });
                    h.dispatch(vec![
                        fixture_open("fixture.log", "one", "one", zone, 0).unwrap(),
                        fixture_open("fixture.log", "two", "two", zone, 1).unwrap(),
                    ])
                    .unwrap();
                    let mut environment = h.runtime.environment().clone();
                    environment.direction = direction;
                    h.runtime.set_environment(environment, &mut h.local).unwrap();
                    let mut raw = RawPanelHost::new(h, scale);
                    raw.capacity.size = if bottom { Size { width: 800.0, height: 1000.0 } } else { Size { width: 1000.0, height: 800.0 } };
                    raw.frame(Vec::new(), false);
                    raw.frame(Vec::new(), false);
                    let cards = &raw.host.frame().unwrap().layout.sections;
                    let axis = |size: Size| if bottom { size.width } else { size.height };
                    assert_eq!((axis(cards[0].outer.size), axis(cards[1].outer.size)), (398.0, 398.0));
                    let at = raw.at(HostControlTarget::Pointer(HostPointerTarget::Sash { first: section("one"), second: section("two") }));
                    raw.pointer(at, true, false);
                    let delta = endpoint - 398.0;
                    let end = if bottom {
                        at + egui::vec2(if direction == Direction::Rtl { -delta } else { delta }, 0.0)
                    } else {
                        at + egui::vec2(0.0, delta)
                    };
                    let shown = raw.frame(vec![egui::Event::PointerMoved(end)], false).0.preview.unwrap().projected.unwrap();
                    assert_eq!((axis(shown.sections[0].outer.size), axis(shown.sections[1].outer.size)), (endpoint, 796.0 - endpoint));
                    raw.pointer(end, false, false);
                    raw.frame(Vec::new(), false);
                    let actual = &raw.host.frame().unwrap().layout;
                    assert_eq!((axis(actual.sections[0].outer.size), axis(actual.sections[1].outer.size)), (endpoint, 796.0 - endpoint));
                    assert!(
                        raw.content
                            .runtime
                            .layout()
                            .layout()
                            .sections
                            .iter()
                            .all(|section| section.preferred_extent.is_none() && section.extent_priority == 0),
                        "ordinary weighted endpoints do not author preference tiers"
                    );
                }
            }
            let mut h = host_with_specs(|spec| {
                spec.min_size.height = 120.0;
                spec.max_size.height = 2000.0;
                if spec.kind.0 == "fixture.editor" {
                    spec.max_size.height = 200.0;
                    spec.default_size.height = 200.0;
                }
            });
            h.dispatch(vec![
                Mutation::Open {
                    kind: PanelKind::new("fixture.editor").unwrap(),
                    instance: id("one"),
                    destination: Some(Destination::NewSection {
                        section: section("one"),
                        index: 0,
                        location: SectionLocation::Dock { zone: Zone::Left },
                        size: Size { width: 280.0, height: 200.0 },
                    }),
                },
                fixture_open("fixture.log", "two", "two", Zone::Left, 1).unwrap(),
                fixture_open("fixture.log", "three", "three", Zone::Left, 2).unwrap(),
            ])
            .unwrap();
            let mut environment = h.runtime.environment().clone();
            environment.direction = direction;
            h.runtime.set_environment(environment, &mut h.local).unwrap();
            let mut raw = RawPanelHost::new(h, scale);
            raw.capacity.size.height = 800.0;
            raw.frame(Vec::new(), false);
            raw.frame(Vec::new(), false);
            let before = raw.host.frame().unwrap().layout.sections.clone();
            assert_eq!(before.iter().map(|card| card.outer.size.height).collect::<Vec<_>>(), vec![200.0, 296.0, 296.0]);
            let at = raw.at(HostControlTarget::Pointer(HostPointerTarget::Sash { first: section("one"), second: section("two") }));
            raw.pointer(at, true, false);
            let end = at - egui::vec2(0.0, 20.0);
            let shown = raw.frame(vec![egui::Event::PointerMoved(end)], false).0.preview.unwrap().projected.unwrap();
            for (card, expected) in shown.sections.iter().take(2).zip([180.0_f32, 316.0]) {
                let actual = card.outer.size.height;
                // The allocator rounds surplus inward when resolving the resized pair.
                assert!(actual.to_bits().abs_diff(expected.to_bits()) <= 2, "actual {actual}, expected {expected} within two f32 ULPs");
            }
            assert_eq!(shown.sections[2].outer, before[2].outer, "unrelated capped allocation stays fixed");
            raw.pointer(end, false, false);
            raw.frame(Vec::new(), false);
            assert_host_landing(&raw.host.frame().unwrap().layout, &shown);
            assert_eq!(raw.host.frame().unwrap().layout.sections[2].outer, before[2].outer, "unrelated capped allocation stays fixed after commit");
            assert!(
                raw.content
                    .runtime
                    .layout()
                    .layout()
                    .sections
                    .iter()
                    .all(|section| section.preferred_extent.is_none() && section.extent_priority == 0)
            );
        }
    }
}

#[test]
fn raw_explicit_extent_survives_collapse_sole_content_and_return_to_stack() {
    let mut h = host_logs(2, Zone::Left);
    h.dispatch(vec![Mutation::SetSize { section: section("section-0"), size: Size { width: 280.0, height: 400.0 } }]).unwrap();
    let weights = h.runtime.layout().layout().sections.iter().map(|section| section.weight).collect::<Vec<_>>();
    let mut raw = RawPanelHost::new(h, 1.0);
    raw.capacity.size.height = 800.0;
    raw.frame(Vec::new(), false);
    raw.frame(Vec::new(), false);
    assert_eq!(raw.host.frame().unwrap().layout.sections.iter().map(|card| card.outer.size.height).collect::<Vec<_>>(), vec![400.0, 396.0]);
    raw.click(HostControlTarget::Pointer(HostPointerTarget::Collapse { instance: id("log-0") }), false);
    raw.frame(Vec::new(), false);
    assert_eq!(raw.host.frame().unwrap().layout.sections[0].outer.size.height, raw.host.frame().unwrap().layout.metrics.header_height);
    raw.click(HostControlTarget::Pointer(HostPointerTarget::Collapse { instance: id("log-0") }), false);
    raw.frame(Vec::new(), false);
    assert_eq!(raw.host.frame().unwrap().layout.sections[0].outer.size.height, 400.0);
    raw.content.dispatch(vec![Mutation::Close { instance: id("log-1") }]).unwrap();
    raw.frame(Vec::new(), false);
    assert_eq!(raw.host.frame().unwrap().layout.sections[0].outer.size.height, 400.0, "a sole panel respects its explicit height");
    assert_eq!(raw.content.runtime.layout().layout().section(&section("section-0")).unwrap().preferred_extent, Some(400.0));
    raw.content.dispatch(vec![Mutation::Open { kind: PanelKind::new("fixture.log").unwrap(), instance: id("log-1"), destination: None }]).unwrap();
    raw.frame(Vec::new(), false);
    assert_eq!(raw.host.frame().unwrap().layout.sections.iter().map(|card| card.outer.size.height).collect::<Vec<_>>(), vec![400.0, 396.0]);
    assert_eq!(raw.content.runtime.layout().layout().sections.iter().map(|section| section.weight).collect::<Vec<_>>(), weights);
    raw.content.dispatch(vec![Mutation::Close { instance: id("log-1") }]).unwrap();
    raw.frame(Vec::new(), false);
    raw.menu(id("log-0"));
    raw.click(HostControlTarget::Shared { instance: id("log-0"), action: HostSharedAction::ResetHeight }, false);
    raw.frame(Vec::new(), false);
    assert_eq!(raw.host.frame().unwrap().layout.sections[0].outer.size.height, 240.0);
    let base = raw.at(HostControlTarget::Pointer(HostPointerTarget::Resize { section: section("section-0"), edge: HostResizeEdge::Bottom }));
    raw.frame(vec![egui::Event::PointerMoved(base)], false);
    raw.pointer(base, true, false);
    let end = base + egui::vec2(0.0, 80.0);
    raw.frame(vec![egui::Event::PointerMoved(end)], false);
    raw.pointer(end, false, false);
    raw.frame(Vec::new(), false);
    assert_eq!(raw.host.frame().unwrap().layout.sections[0].outer.size.height, 320.0);
}

#[test]
fn raw_close_keeps_original_lifecycle_intent_and_refuses_a_live_editor_guard() {
    let mut h = host_with_specs(|spec| spec.features.hot_closable = true);
    open_editor(&mut h);
    let key = editor_key(&h);
    h.probe
        .edit(&key, |state| {
            state.reject_cancel = true;
            state.draft = "guarded retained transaction".into();
        })
        .unwrap();
    let mut raw = RawPanelHost::new(h, 1.0);
    raw.frame(Vec::new(), false);
    raw.frame(Vec::new(), false);
    let before = raw.content.runtime.layout().inspect();
    raw.click(HostControlTarget::Pointer(HostPointerTarget::Close { instance: id("editor") }), false);
    assert_eq!(raw.content.runtime.layout().inspect(), before);
    assert_eq!(raw.content.probe.state(&key).unwrap().cancels, 0);
    assert_eq!(raw.content.probe.state(&key).unwrap().draft, "guarded retained transaction");
    assert!(raw.host.frame().unwrap().events.iter().any(|event| matches!(event, HostEvent::Rejected { error, .. } if error.rule == Rule::Lifecycle)));
}

#[test]
fn raw_tiny_scroll_uses_real_card_layer_and_uncovered_content_gap_stays_navigable() {
    for scale in [1.0, 2.0] {
        for direction in [Direction::Ltr, Direction::Rtl] {
            let mut content = host_logs(4, Zone::Left);
            let mut environment = content.runtime.environment().clone();
            environment.direction = direction;
            content.runtime.set_environment(environment, &mut content.local).unwrap();
            let mut raw = RawPanelHost::new(content, scale);
            raw.capacity.size = Size { width: 600.0, height: 180.0 };
            raw.frame(Vec::new(), false);
            raw.frame(Vec::new(), false);
            let frame = raw.host.frame().unwrap();
            assert_eq!(frame.layout.sections.len(), 4);
            assert!(frame.layout.extent.height >= 4.0 * 120.0 + 12.0);
            let at = egui_rect(frame.layout.sections[0].outer).center();
            let before = frame.layout.geometry_policy.scroll.y;
            raw.frame(
                vec![
                    egui::Event::PointerMoved(at),
                    egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: egui::vec2(0.0, -72.0),
                        modifiers: egui::Modifiers::NONE,
                        phase: egui::TouchPhase::Move,
                        source: egui::MouseWheelSource::Unknown,
                    },
                ],
                false,
            );
            raw.frame(Vec::new(), false);
            assert!(raw.host.frame().unwrap().layout.geometry_policy.scroll.y > before, "real Area-owned wheel reaches the host canvas");
            assert!(raw.host.frame().unwrap().layout.input.iter().all(|bounds| intersect_rect(raw.capacity, *bounds) == Some(*bounds)));
            let mut sole = RawPanelHost::new(host_logs(1, Zone::Left), scale);
            let mut environment = sole.content.runtime.environment().clone();
            environment.direction = direction;
            sole.content.runtime.set_environment(environment, &mut sole.content.local).unwrap();
            sole.frame(Vec::new(), false);
            sole.frame(Vec::new(), false);
            let card = sole.host.frame().unwrap().layout.sections[0].outer;
            let gap = Point { x: middle(card).x, y: card.origin.y + card.size.height + 24.0 };
            let frame = sole.host.frame().unwrap();
            assert!(frame.layout.usable.contains_rect(LogicalRect { origin: gap, size: Size { width: 4.0, height: 4.0 } }));
            assert!(!frame.layout.input.iter().any(|bounds| bounds.contains(gap)));
            assert_eq!(frame.layout.viewport, Some(sole.capacity));
        }
    }
}

#[test]
#[ignore = "opt-in resolver measurement; root records machine/profile after source review"]
fn host_resolver_measurement_exercise_for_twenty_four_panels() {
    use std::hint::black_box;
    use std::time::Instant;
    let mut h = host_logs(24, Zone::Left);
    for index in 1..24 {
        if index % 6 == 0 {
            h.dispatch(vec![Mutation::Move {
                instance: id(&format!("log-{index}")),
                destination: Destination::Relative {
                    section: section(&format!("section-{}", index - 1)),
                    placement: RelativePlacement::AfterColumn,
                    new_section: section(&format!("section-{index}")),
                    new_column: Some(ColumnId::new(format!("measurement.column.{index}")).unwrap()),
                    size: Size { width: 280.0, height: 240.0 },
                },
            }])
            .unwrap();
        }
        if index % 5 == 0 {
            h.dispatch(vec![Mutation::Collapse { section: section(&format!("section-{index}")), collapsed: true }]).unwrap();
        }
    }
    for index in [1, 9, 17] {
        h.dispatch(vec![Mutation::Open {
            kind: PanelKind::new("fixture.log").unwrap(),
            instance: id(&format!("measurement.tab.{index}")),
            destination: Some(Destination::JoinTab { section: section(&format!("section-{index}")), index: 1 }),
        }])
        .unwrap();
    }
    let snapshot_started = Instant::now();
    let provider = h.runtime.capture_metrics(&h.models).unwrap();
    let reservations = fixture_complete_reservations(h.runtime.layout());
    let policy = HostGeometryPolicy {
        content_revision: provider.revision(),
        measure: Some(provider),
        identity_reservations: reservations,
        ..Default::default()
    };
    let snapshot_cost = snapshot_started.elapsed();
    let before = h.runtime.layout().inspect();
    let domain = LogicalRect { origin: Point { x: 0.0, y: 32.0 }, size: Size { width: 1800.0, height: 1000.0 } };
    let capacity = LogicalRect { origin: Point { x: 0.0, y: 96.0 }, size: Size { width: 1800.0, height: 936.0 } };
    let chrome = [LogicalRect { origin: domain.origin, size: Size { width: 1800.0, height: 64.0 } }];
    for direction in [Direction::Ltr, Direction::Rtl] {
        let mut samples = Vec::new();
        for _ in 0..200 {
            let started = Instant::now();
            let layout = black_box(
                resolve_host_layout_in_domain(
                    black_box(h.runtime.layout()),
                    domain,
                    capacity,
                    &chrome,
                    direction,
                    HostMetrics::default(),
                    black_box(&policy),
                )
                .unwrap(),
            );
            samples.push(started.elapsed());
            assert_eq!(layout.sections.len(), 24);
            assert_eq!(layout.viewport, Some(domain));
        }
        samples.sort();
        eprintln!(
            "resolver measurement os={} arch={} profile={} direction={direction:?} samples={} median={:?} range={:?}..{:?} snapshot={snapshot_cost:?}; observational target <0.2ms, no timing gate",
            std::env::consts::OS,
            std::env::consts::ARCH,
            if cfg!(debug_assertions) { "debug" } else { "release" },
            samples.len(),
            samples[samples.len() / 2],
            samples.first(),
            samples.last()
        );
    }
    assert_eq!(h.runtime.layout().inspect(), before);
}

#[test]
fn cached_command_proposal_is_discarded_on_metric_change_without_a_second_factory() {
    let (mut h, trace) = review_cohort_host(|_| {});
    h.dispatch(vec![fixture_open("fixture.log", "log", "log", Zone::Left, 0).unwrap()]).unwrap();
    h.tick().unwrap();
    let mut host = WorkspacePanelHost::new(HostOptions::default()).unwrap();
    host.capture_metrics(&h.runtime, &h.models).unwrap();
    host.capture_reservations(&h.runtime, &h.catalogue).unwrap();
    host.prepare(&h.runtime, capacity()).unwrap();
    let action = transaction(vec![Mutation::Collapse { section: section("log"), collapsed: true }]);
    let before = h.runtime.layout().inspect();
    let factories = trace.borrow().factories.len();
    let mut services = ContentServices { models: &h.models, commands: &mut h.commands, local: &mut h.local };
    {
        let mut live = RuntimeActionSink { runtime: &mut h.runtime, native: &mut h.native, services: &mut services };
        let mut sink = super::app::AdmittedActionSink { live: &mut live, host: Some(&mut host), proposal: std::cell::RefCell::new(None) };
        sink.project(action.clone()).unwrap();
        trace.borrow_mut().metric_revision += 1;
        sink.host.as_mut().unwrap().capture_metrics(sink.live.runtime, sink.live.services.models).unwrap();
        assert_eq!(sink.dispatch(action).unwrap_err().rule, Rule::Revision);
        assert!(sink.proposal.borrow().is_none());
    }
    assert_eq!(h.runtime.layout().inspect(), before);
    assert_eq!(trace.borrow().factories.len(), factories);
}

#[test]
fn command_storage_refusal_uses_the_same_host_admission_before_any_runtime_publication() {
    let (mut h, trace) = review_cohort_host(|_| {});
    h.dispatch(vec![fixture_open("fixture.log", "log", "log", Zone::Left, 0).unwrap()]).unwrap();
    h.tick().unwrap();
    let mut host = WorkspacePanelHost::new(HostOptions::default()).unwrap();
    host.capture_metrics(&h.runtime, &h.models).unwrap();
    host.capture_reservations(&h.runtime, &h.catalogue).unwrap();
    host.prepare(&h.runtime, capacity()).unwrap();
    let before = h.runtime.layout().inspect();
    let controllers = h.runtime.controller_count();
    let factories = trace.borrow().factories.len();
    let commits = h.native.commits;
    h.catalogue.fail_stage = true;
    let registry = h.runtime.layout().registry().clone();
    let mut services = ContentServices { models: &h.models, commands: &mut h.commands, local: &mut h.local };
    {
        let mut live = RuntimeActionSink { runtime: &mut h.runtime, native: &mut h.native, services: &mut services };
        let mut sink = super::app::AdmittedActionSink { live: &mut live, host: Some(&mut host), proposal: std::cell::RefCell::new(None) };
        assert_eq!(
            commands::execute_layout_action(
                transaction(vec![Mutation::Collapse { section: section("log"), collapsed: true }]),
                &mut commands::PanelCommandContext { sink: &mut sink, registry: &registry, catalogue: &mut h.catalogue, web: false }
            )
            .unwrap_err()
            .rule,
            Rule::Capability
        );
    }
    assert_eq!(h.runtime.layout().inspect(), before);
    assert_eq!(h.runtime.controller_count(), controllers);
    assert_eq!(trace.borrow().factories.len(), factories);
    assert_eq!(h.native.commits, commits);
}

#[test]
fn standalone_registered_action_needs_neither_host_environment_nor_layout_store() {
    let (mut h, trace) = review_cohort_host(|_| {});
    h.dispatch(vec![fixture_open("fixture.log", "log", "log", Zone::Left, 0).unwrap()]).unwrap();
    let mut app = crate::SolveApp::new(solidcraft_engine::Session::default(), Default::default());
    app.enable_panel_runtime(h.runtime, Box::new(FakeAppPanelServices { models: FakeModels { editable: true }, ..Default::default() })).unwrap();
    let mut catalogue = commands::InMemoryCatalogue::default();
    catalogue.fail_stage = true;
    app.panel_runtime_mut().unwrap().set_command_adapters(Box::new(catalogue), Box::new(InAppNativeAdapter)).unwrap();
    app.run_panel_command("panel.action", &serde_json::json!({"target":{"type":"instance","id":"log"},"action":"pulse"})).unwrap();
    assert_eq!(trace.borrow().actions.len(), 1);
    assert!(!app.universal_panel_host_enabled());
    assert_eq!(
        app.dispatch_panel_layout(transaction(vec![Mutation::SetSize { section: section("log"), size: Size { width: 280.0, height: 300.0 } }]))
            .unwrap_err()
            .rule,
        Rule::Availability
    );
    assert_eq!(
        app.dispatch_panel_layout(transaction(vec![
            Mutation::HeaderAction { instance: id("log"), action: ControlId::new("pulse").unwrap() },
            Mutation::Collapse { section: section("log"), collapsed: true }
        ]))
        .unwrap_err()
        .rule,
        Rule::Capability
    );
    assert_eq!(trace.borrow().actions.len(), 1);
}

#[test]
fn raw_inline_title_actions_editor_and_system_slots_mirror_and_keep_editor_identity() {
    for scale in [1.0, 2.0] {
        for direction in [Direction::Ltr, Direction::Rtl] {
            let (mut h, trace) = review_cohort_host(|spec| {
                spec.min_size.height = 40.0;
            });
            h.dispatch(vec![
                fixture_open("fixture.editor", "inline-editor", "inline", Zone::Bottom, 0).unwrap(),
                Mutation::SetSize { section: section("inline"), size: Size { width: 800.0, height: 64.0 } },
            ])
            .unwrap();
            let mut environment = h.runtime.environment().clone();
            environment.direction = direction;
            h.runtime.set_environment(environment, &mut h.local).unwrap();
            let mut raw = RawPanelHost::new(h, scale);
            raw.frame(Vec::new(), false);
            raw.frame(Vec::new(), false);
            let card = raw.host.frame().unwrap().layout.sections[0].clone();
            assert!(card.inline && card.body.is_none() && card.footer.is_none());
            assert_eq!(card.outer.size.height, 64.0);
            let shared_title = card.title.expect("inline composition retains the actual shared title/grip");
            let slots = [shared_title, card.actions[0].1, card.inline_content.unwrap(), card.menu, card.collapse.unwrap(), card.close.unwrap()];
            for pair in slots.windows(2) {
                if direction == Direction::Ltr {
                    assert!(pair[0].origin.x + pair[0].size.width <= pair[1].origin.x);
                } else {
                    assert!(pair[1].origin.x + pair[1].size.width <= pair[0].origin.x);
                }
            }
            let title = raw_control(&raw, &HostControlTarget::Pointer(HostPointerTarget::Header { instance: id("inline-editor") }));
            assert_eq!(title.bounds, shared_title, "actual shared title/grip keeps the dedicated slot");
            let start = egui_rect(title.bounds).center();
            raw.pointer(start, true, false);
            raw.frame(vec![egui::Event::PointerMoved(start + egui::vec2(6.0, 0.0))], false);
            assert!(raw.host.interaction().is_captured());
            raw.frame(vec![host_key(egui::Key::Escape, true), button_event(start, false)], false);
            raw.frame(vec![host_key(egui::Key::Escape, false)], false);
            let (editor_id, _, bounds, _) = trace.borrow().editors.last().unwrap().clone();
            let at = bounds.center();
            raw.pointer(at, true, false);
            raw.frame(vec![egui::Event::PointerMoved(at + egui::vec2(12.0, 0.0))], false);
            assert!(!raw.host.interaction().is_captured(), "editing never starts title drag");
            raw.pointer(at + egui::vec2(12.0, 0.0), false, false);
            raw.ctx.memory_mut(|memory| memory.request_focus(editor_id));
            raw.frame(vec![egui::Event::Text(" changed".into())], false);
            let caret = egui::TextEdit::load_state(&raw.ctx, editor_id).unwrap().cursor.char_range();
            let draft = trace.borrow().editors.last().unwrap().3.clone();
            assert!(draft.contains("changed"));
            let count = raw.content.runtime.controller_count();
            raw.content
                .dispatch(vec![Mutation::Move {
                    instance: id("inline-editor"),
                    destination: Destination::NewSection {
                        section: section("inline"),
                        index: 0,
                        location: SectionLocation::Dock { zone: Zone::Right },
                        size: Size { width: 280.0, height: 300.0 },
                    },
                }])
                .unwrap();
            raw.frame(Vec::new(), false);
            assert!(!raw.host.frame().unwrap().layout.sections[0].inline);
            assert_eq!(trace.borrow().editors.last().unwrap().0, editor_id);
            assert_eq!(trace.borrow().editors.last().unwrap().3, draft);
            assert_eq!(egui::TextEdit::load_state(&raw.ctx, editor_id).unwrap().cursor.char_range(), caret);
            assert_eq!(raw.content.runtime.controller_count(), count);

            // The same retained editor also composes with header tabs. Give
            // both labels room, retaining actual editable inline content.
            let mut options = HostOptions::default();
            options.metrics.inline_title_width = 256.0;
            raw.host = WorkspacePanelHost::new(options).unwrap();
            raw.content
                .dispatch(vec![
                    Mutation::Move {
                        instance: id("inline-editor"),
                        destination: Destination::NewSection {
                            section: section("inline"),
                            index: 0,
                            location: SectionLocation::Dock { zone: Zone::Bottom },
                            size: Size { width: 800.0, height: 64.0 },
                        },
                    },
                    Mutation::Open {
                        kind: PanelKind::new("fixture.log").unwrap(),
                        instance: id("inline-peer"),
                        destination: Some(Destination::JoinTab { section: section("inline"), index: 1 }),
                    },
                    Mutation::Activate { instance: id("inline-editor") },
                ])
                .unwrap();
            raw.frame(Vec::new(), false);
            raw.frame(Vec::new(), false);
            let card = raw.host.frame().unwrap().layout.sections[0].clone();
            assert!(card.inline && card.inline_content.is_some());
            assert_eq!(card.header, card.outer);
            assert_eq!(card.tabs.len(), 2);
            assert_host_geometry(&raw.content, &raw.host.frame().unwrap().layout);
            assert_eq!(trace.borrow().editors.last().unwrap().0, editor_id);
            assert_eq!(trace.borrow().editors.last().unwrap().3, draft);
            let grip = HostControlTarget::Pointer(HostPointerTarget::Header { instance: id("inline-editor") });
            let start = raw.at(grip);
            raw.pointer(start, true, false);
            raw.frame(vec![egui::Event::PointerMoved(start + egui::vec2(6.0, 0.0))], false);
            assert!(raw.host.interaction().is_captured(), "the separate group grip drags the active tab");
            raw.frame(vec![host_key(egui::Key::Escape, true), button_event(start, false)], false);
            raw.frame(vec![host_key(egui::Key::Escape, false)], false);
            let start = raw.at(HostControlTarget::Pointer(HostPointerTarget::Tab { instance: id("inline-peer") }));
            let first = raw.host.frame().unwrap().layout.sections[0].tabs[0].bounds;
            let end = egui::pos2(first.origin.x + first.size.width * if direction == Direction::Ltr { 0.25 } else { 0.75 }, middle(first).y);
            raw.pointer(start, true, false);
            let preview = raw.frame(vec![egui::Event::PointerMoved(end)], false).0.preview.unwrap();
            assert!(preview.rejection.is_none());
            assert_eq!(preview.destination, Some(Destination::JoinTab { section: section("inline"), index: 0 }));
            let shown = preview.projected.unwrap();
            let before = raw.content.runtime.layout().inspect();
            let commits = raw.content.native.commits;
            raw.pointer(end, false, false);
            raw.frame(Vec::new(), false);
            assert_host_landing(&raw.host.frame().unwrap().layout, &shown);
            assert_eq!(raw.content.runtime.layout().inspect().revision, before.revision + 1);
            assert_eq!(raw.content.native.commits, commits + 1);
            assert_eq!(raw.content.runtime.layout().layout().sections[0].tabs, vec![id("inline-peer"), id("inline-editor")]);
            raw.click(HostControlTarget::Pointer(HostPointerTarget::Collapse { instance: id("inline-peer") }), false);
            raw.frame(Vec::new(), false);
            let card = &raw.host.frame().unwrap().layout.sections[0];
            assert!(card.collapsed && card.body.is_none());
            assert_eq!(card.header, card.outer);
            assert_eq!(card.tabs.len(), 2);
            raw.click(HostControlTarget::Pointer(HostPointerTarget::Tab { instance: id("inline-editor") }), false);
            raw.frame(Vec::new(), false);
            assert!(raw.host.frame().unwrap().layout.sections[0].collapsed);
            assert_eq!(raw.content.runtime.layout().layout().sections[0].active, id("inline-editor"));
            raw.click(HostControlTarget::Pointer(HostPointerTarget::Collapse { instance: id("inline-editor") }), false);
            raw.frame(Vec::new(), false);
            assert!(raw.host.frame().unwrap().layout.sections[0].inline);
            assert_eq!(trace.borrow().editors.last().unwrap().0, editor_id);
            assert_eq!(trace.borrow().editors.last().unwrap().3, draft);
            assert_eq!(raw.content.runtime.controller_count(), count + 1);
        }
    }
}

#[test]
fn zero_saved_stack_weight_is_transiently_content_or_fill_when_sole() {
    for fill in [false, true] {
        let mut service = allocation_service(|spec| {
            spec.side_height_policy = if fill { SideHeightPolicy::Fill } else { SideHeightPolicy::Content };
        });
        apply_fixture(
            &mut service,
            vec![
                fixture_open("fixture.log", "one", "one", Zone::Left, 0).unwrap(),
                fixture_open("fixture.log", "two", "two", Zone::Left, 1).unwrap(),
            ],
        );
        let host = allocate_fixture(&service, Size { width: 1000.0, height: 800.0 }, Direction::Ltr, &fixture_metric_policy(&service));
        let a = host.sections[0].outer.size;
        let b = host.sections[1].outer.size;
        let candidate = prepare_host_layout(
            &service,
            &fixture_scope(&service),
            &host,
            transaction(vec![Mutation::ResizePair {
                first: section("one"),
                second: section("two"),
                first_size: Size { height: 120.0, ..a },
                second_size: Size { height: 676.0, ..b },
            }]),
        )
        .unwrap();
        fixture_commit(&mut service, &host, candidate);
        let saved = service.layout().section(&section("one")).unwrap().weight;
        assert_eq!(saved, 0.0);
        apply_fixture(&mut service, vec![Mutation::Close { instance: id("two") }]);
        let sole = allocate_fixture(&service, Size { width: 1000.0, height: 800.0 }, Direction::Ltr, &fixture_metric_policy(&service));
        assert_eq!(sole.sections[0].outer.size.height, if fill { 800.0 } else { 240.0 });
        assert_eq!(service.layout().section(&section("one")).unwrap().weight, saved);
        apply_fixture(&mut service, vec![Mutation::Open { kind: PanelKind::new("fixture.log").unwrap(), instance: id("two"), destination: None }]);
        let stacked = allocate_fixture(&service, Size { width: 1000.0, height: 800.0 }, Direction::Ltr, &fixture_metric_policy(&service));
        assert_eq!(stacked.sections.iter().map(|card| card.outer.size.height).collect::<Vec<_>>(), vec![120.0, 676.0]);
    }
}

#[test]
fn raw_full_envelope_reservations_skip_quarantine_and_native_return_ids_and_cancel_idle_capture() {
    let mut h = host_logs(2, Zone::Left);
    h.dispatch(vec![
        Mutation::Open {
            kind: PanelKind::new("fixture.log").unwrap(),
            instance: id("moving"),
            destination: Some(Destination::JoinTab { section: section("section-0"), index: 1 }),
        },
        Mutation::Activate { instance: id("moving") },
    ])
    .unwrap();
    let reserved_section = next_host_section_id(h.runtime.layout().layout(), &HostIdentityReservations::default()).unwrap();
    let reserved_column = next_host_column_id(h.runtime.layout().layout(), &HostIdentityReservations::default()).unwrap();
    let mut envelope = LayoutEnvelope::new(h.runtime.layout().layout().clone());
    let pending = serde_json::json!({"id":"extension","kind":"extension.panel","requested_open":false,"memory":{
    "section":reserved_section,"section_index":2,"tab_index":0,"size":{"width":280,"height":240},"collapsed":false,"weight":1,
    "preferred_extent":null,"column":null,"column_index":0,"column_width":null,
    "location":{"type":"window","host":"return-host","origin":{"x":20,"y":30},"return_to":{
        "section":"review.return","zone":"left","index":2,"column":reserved_column,"column_index":1,"column_width":280,
        "size":{"width":280,"height":240},"collapsed":false,"weight":1,"preferred_extent":null
    }}}});
    envelope.quarantine.push(QuarantinedPanel {
        instance: id("extension"),
        kind: PanelKind::new("extension.panel").unwrap(),
        instance_data: pending,
        section: None,
    });
    let ownership = ReplacementIdentityOwnership::from_layout(&envelope.layout).unwrap();
    let mut captured = host_identity_reservations(&envelope).unwrap();
    assert_eq!(captured.replacement_ownership.as_ref(), Some(&ownership));
    assert_eq!(captured.preparation_context().unwrap().replacement_ownership.as_ref(), Some(&ownership));
    assert!(!ownership.sections.contains_key(&reserved_section));
    assert!(!ownership.columns.contains_key(&reserved_column));
    let mut other = allocation_service(|_| {});
    apply_fixture(&mut other, vec![fixture_open("fixture.log", "other", "other", Zone::Right, 0).unwrap()]);
    captured.include_layout(other.layout()).unwrap();
    assert!(captured.sections.contains(&section("other")));
    assert_eq!(captured.replacement_ownership.as_ref(), Some(&ownership), "live reservations cannot grant incoming ownership");
    h.catalogue.remember(&envelope, None, h.runtime.layout().registry()).unwrap();
    let mut raw = RawPanelHost::new(h, 1.0);
    raw.frame(Vec::new(), false);
    raw.frame(Vec::new(), false);
    let reservations = &raw.host.frame().unwrap().layout.geometry_policy.identity_reservations;
    assert!(reservations.sections.contains(&reserved_section) && reservations.columns.contains(&reserved_column));
    let start = raw.at(HostControlTarget::Pointer(HostPointerTarget::Header { instance: id("moving") }));
    let landing = explicit_drop_point(
        &raw.content,
        &raw.host.frame().unwrap().layout,
        &id("moving"),
        &HostDropTargetId::Relative { section: section("section-1"), placement: RelativePlacement::AfterColumn },
    );
    let at = egui::pos2(landing.x, landing.y);
    raw.pointer(start, true, false);
    let shown = raw.frame(vec![egui::Event::PointerMoved(at)], false).0.preview.unwrap();
    let action = shown.action.clone().unwrap();
    if let LayoutAction::Apply { mutations, .. } = &action {
        let destination = mutations
            .iter()
            .find_map(|mutation| match mutation {
                Mutation::Move { destination, .. } => Some(destination),
                _ => None,
            })
            .unwrap();
        let Destination::Relative { new_section, new_column, .. } = destination else {
            panic!("relative column landing required");
        };
        assert_ne!(new_section, &reserved_section);
        assert_ne!(new_column.as_ref(), Some(&reserved_column));
    }
    assert_eq!(raw.frame(vec![egui::Event::PointerMoved(at)], false).0.preview.unwrap().action, Some(action));
    raw.pointer(at, false, false);
    raw.frame(Vec::new(), false);
    let before = raw.content.runtime.layout().inspect();
    let start = raw.at(HostControlTarget::Pointer(HostPointerTarget::Header { instance: id("moving") }));
    raw.pointer(start, true, false);
    raw.frame(vec![egui::Event::PointerMoved(start + egui::vec2(6.0, 0.0))], false);
    assert!(raw.host.interaction().is_captured());
    let mut changed = LayoutEnvelope::new(before.layout.clone());
    changed.quarantine = envelope.quarantine;
    changed.quarantine[0].instance_data["memory"]["location"]["return_to"]["column"] = serde_json::json!("review.changed.return.column");
    raw.content.catalogue.remember(&changed, None, raw.content.runtime.layout().registry()).unwrap();
    raw.frame(Vec::new(), false);
    assert!(!raw.host.interaction().is_captured());
    assert!(raw.host.frame().unwrap().preview.is_none());
    assert_eq!(raw.content.runtime.layout().inspect(), before);
}

#[test]
fn atomic_batch_growing_projections_reuse_one_private_cohort_and_measure_each_new_controller_once() {
    let (mut h, trace) = review_cohort_host(|_| {});
    let mut host = WorkspacePanelHost::new(HostOptions::default()).unwrap();
    host.capture_metrics(&h.runtime, &h.models).unwrap();
    host.capture_reservations(&h.runtime, &h.catalogue).unwrap();
    host.prepare(&h.runtime, capacity()).unwrap();
    let registry = h.runtime.layout().registry().clone();
    let mut services = ContentServices { models: &h.models, commands: &mut h.commands, local: &mut h.local };
    {
        let mut live = RuntimeActionSink { runtime: &mut h.runtime, native: &mut h.native, services: &mut services };
        let mut sink = super::app::AdmittedActionSink { live: &mut live, host: Some(&mut host), proposal: std::cell::RefCell::new(None) };
        let result = commands::execute_batch(
            &[
                serde_json::json!({"command":"panel.open","params":{"kind":"fixture.log","instance":"one","destination":{
                "type":"new_section","section":"one","index":0,"location":{"type":"dock","zone":"left"},"size":{"width":280,"height":240}}}}),
                serde_json::json!({"command":"panel.open","params":{"kind":"fixture.log","instance":"two","destination":{
                "type":"new_section","section":"two","index":1,"location":{"type":"dock","zone":"left"},"size":{"width":280,"height":240}}}}),
                serde_json::json!({"command":"layout.get"}),
            ],
            &mut commands::PanelCommandContext { sink: &mut sink, registry: &registry, catalogue: &mut h.catalogue, web: false },
        )
        .unwrap();
        assert_eq!(result["completed"], 3);
    }
    assert_eq!(trace.borrow().factories.len(), 2);
    assert_eq!(trace.borrow().measures, vec![1, 2]);
    assert_eq!(h.runtime.controller_count(), 2);
    assert_eq!(h.runtime.layout().layout().sections.len(), 2);
    assert_eq!(h.native.commits, 1, "only the final owned candidate commits");
}

// Immutable-provider drift is injected inside the real catalogue boundary. This
// fixture intentionally uses PanelController's default live revision fallback.
#[derive(Default)]
struct ImmutableSourceTrace {
    factories: Vec<(ControllerKey, usize)>,
    measured: Vec<(ControllerKey, usize)>,
}
type ImmutableTrace = std::rc::Rc<std::cell::RefCell<ImmutableSourceTrace>>;
struct SharedHeightModels(std::rc::Rc<std::cell::Cell<u64>>);
impl PanelModels for SharedHeightModels {
    fn read(&self, _owner: &ContentOwner, key: &str) -> PanelResult<serde_json::Value> {
        match key {
            "review.height" => Ok(serde_json::json!(self.0.get())),
            "fixture.editable" => Ok(serde_json::json!(true)),
            _ => Err(error(Rule::MissingTarget, "unknown measurement fixture model")),
        }
    }
}
struct ImmutableSourceController {
    inner: Box<dyn PanelController>,
    key: ControllerKey,
    identity: usize,
    trace: ImmutableTrace,
}
impl PanelController for ImmutableSourceController {
    fn availability(&self, owner: &ContentOwner, models: &dyn PanelModels) -> Availability {
        self.inner.availability(owner, models)
    }
    fn update(&mut self, owner: &ContentOwner, services: &mut ContentServices<'_>) -> PanelResult<()> {
        self.inner.update(owner, services)
    }
    fn presentation(&self) -> PanelPresentation {
        self.inner.presentation()
    }
    fn measure(&self, owner: &ContentOwner, models: &dyn PanelModels, spec: &PanelSpec) -> PanelResult<std::sync::Arc<dyn HostContentMetrics>> {
        let height = models.read(owner, "review.height")?.as_u64().ok_or_else(|| error(Rule::Bounds, "height fixture requires an integer"))?;
        self.trace.borrow_mut().measured.push((self.key.clone(), self.identity));
        Ok(std::sync::Arc::new(ReviewMetrics { revision: height, height: height as f32, policy: spec.side_height_policy }))
    }
    fn prepare_lifecycle(&self, event: &LifecycleEvent) -> PanelResult<()> {
        self.inner.prepare_lifecycle(event)
    }
    fn lifecycle(&mut self, event: &LifecycleEvent) {
        self.inner.lifecycle(event);
    }
    fn render(&mut self, slot: ContentSlot, ui: &mut egui::Ui, context: &PanelContext, services: &mut ContentServices<'_>) -> PanelResult<()> {
        self.inner.render(slot, ui, context, services)
    }
    fn action(&mut self, owner: &ContentOwner, action: &ControlId, services: &mut ContentServices<'_>) -> PanelResult<()> {
        self.inner.action(owner, action, services)
    }
}
struct SharedHeightServices {
    models: SharedHeightModels,
    commands: FakeContentCommands,
    local: FakeLocalServices,
}
impl AppPanelServices for SharedHeightServices {
    fn refresh_read_only(&mut self, _session: &solidcraft_engine::Session) -> PanelResult<()> {
        Ok(())
    }
    fn services(&mut self) -> ContentServices<'_> {
        ContentServices { models: &self.models, commands: &mut self.commands, local: &mut self.local }
    }
}
struct SourceDriftCatalogue {
    inner: commands::InMemoryCatalogue,
    source: std::rc::Rc<std::cell::Cell<u64>>,
    change_to: Option<u64>,
    prepared: std::rc::Rc<std::cell::Cell<usize>>,
    published: std::rc::Rc<std::cell::Cell<usize>>,
    observed: std::rc::Rc<std::cell::RefCell<commands::CatalogueState>>,
}
impl commands::LayoutCatalogue for SourceDriftCatalogue {
    fn state(&self) -> &commands::CatalogueState {
        commands::LayoutCatalogue::state(&self.inner)
    }
    fn prepare(&mut self, next: commands::CatalogueState) -> PanelResult<commands::PreparedCatalogue> {
        self.prepared.set(self.prepared.get() + 1);
        let prepared = commands::LayoutCatalogue::prepare(&mut self.inner, next)?;
        if let Some(height) = self.change_to {
            self.source.set(height);
        }
        *self.observed.borrow_mut() = commands::LayoutCatalogue::state(&self.inner).clone();
        Ok(prepared)
    }
    fn publish(&mut self, prepared: commands::PreparedCatalogue) {
        self.published.set(self.published.get() + 1);
        commands::LayoutCatalogue::publish(&mut self.inner, prepared);
        *self.observed.borrow_mut() = commands::LayoutCatalogue::state(&self.inner).clone();
    }
}

#[test]
fn immutable_metric_drift_inside_catalogue_prepare_rejects_retained_and_staged_owner_without_publication() {
    use commands::LayoutCatalogue;
    for incoming_owner in [false, true] {
        for drift in [false, true] {
            let h = PanelMountHarness::without_content(false).unwrap();
            let mut service = fixture_service(false).unwrap();
            apply_fixture(&mut service, vec![fixture_open("fixture.log", "log", "log", Zone::Left, 0).unwrap()]);
            let mut catalogue = fixture_command_catalogue(service.registry()).unwrap();
            let mut incoming = service.layout().clone();
            incoming.workspace = WorkspaceId::new("fixture.other").unwrap();
            catalogue.remember(&LayoutEnvelope::new(incoming), None, service.registry()).unwrap();
            let catalogue_before = catalogue.state().clone();
            let observed = std::rc::Rc::new(std::cell::RefCell::new(catalogue_before.clone()));
            let prepared = std::rc::Rc::new(std::cell::Cell::new(0));
            let published = std::rc::Rc::new(std::cell::Cell::new(0));
            let source = std::rc::Rc::new(std::cell::Cell::new(240));
            let trace = ImmutableTrace::default();
            let mut runtime = PanelRuntime::new(service, h.runtime.environment().clone()).unwrap();
            let binding = fixture_controller_binding(&h.probe, false, Ownership::Workspace);
            let create = binding.create;
            let factory_trace = trace.clone();
            runtime
                .register(
                    PanelKind::new("fixture.log").unwrap(),
                    ControllerBinding {
                        ownership: Ownership::Workspace,
                        create: Box::new(move |instance, owner| {
                            let key = ControllerKey { instance: instance.id.clone(), owner: owner.clone() };
                            let identity = factory_trace.borrow().factories.len() + 1;
                            let inner = create(instance, owner)?;
                            factory_trace.borrow_mut().factories.push((key.clone(), identity));
                            Ok(Box::new(ImmutableSourceController { inner, key, identity, trace: factory_trace.clone() }))
                        }),
                    },
                )
                .unwrap();
            let mut app = crate::SolveApp::new(solidcraft_engine::Session::default(), Default::default());
            app.enable_panel_runtime(
                runtime,
                Box::new(SharedHeightServices {
                    models: SharedHeightModels(source.clone()),
                    commands: Default::default(),
                    local: Default::default(),
                }),
            )
            .unwrap();
            let native = std::rc::Rc::new(std::cell::RefCell::new(FakeNativeAdapter::new(false)));
            app.panel_runtime_mut()
                .unwrap()
                .set_command_adapters(
                    Box::new(SourceDriftCatalogue {
                        inner: catalogue,
                        source: source.clone(),
                        change_to: drift.then_some(360),
                        prepared: prepared.clone(),
                        published: published.clone(),
                        observed: observed.clone(),
                    }),
                    Box::new(SharedCommandNative(native.clone())),
                )
                .unwrap();
            prepare_app_commands(&mut app);
            let before = app.panel_runtime().unwrap().layout().inspect();
            let count = app.panel_runtime().unwrap().controller_count();
            let factories = trace.borrow().factories.len();
            let doc = app.session.doc.clone();
            let model = app.session.model.state();
            let revision = app.session.revision;
            let undo = app.session.undo.len();
            let dirty = app.session.is_dirty();
            let result = if incoming_owner {
                app.run_panel_command("layout.workspace.select", &serde_json::json!({"workspace":"fixture.other"}))
            } else {
                app.run_panel_command(
                    "panel.move",
                    &serde_json::json!({"target":{"type":"instance","id":"log"},"destination":{
                    "type":"new_section","section":"log","index":0,"location":{"type":"dock","zone":"right"},"size":{"width":280,"height":240}}}),
                )
            };
            assert_eq!(prepared.get(), 1, "the injected real catalogue boundary was exercised");
            if drift {
                assert_eq!(result.unwrap_err().rule, Rule::Revision);
                assert_eq!(app.panel_runtime().unwrap().layout().inspect(), before);
                assert_eq!(app.panel_runtime().unwrap().controller_count(), count);
                assert_eq!(*observed.borrow(), catalogue_before);
                assert_eq!(published.get(), 0);
                assert_eq!(native.borrow().commits, 0);
                assert!(native.borrow().hosts.is_empty());
            } else {
                result.unwrap();
                assert_eq!(published.get(), 1);
                assert_eq!(native.borrow().commits, 1);
            }
            assert_eq!(trace.borrow().factories.len(), factories + usize::from(incoming_owner));
            let target = ControllerKey {
                instance: id("log"),
                owner: ContentOwner::Workspace(WorkspaceId::new(if incoming_owner { "fixture.other" } else { "fixture.workspace" }).unwrap()),
            };
            let identity = trace.borrow().factories.iter().find(|(key, _)| *key == target).unwrap().1;
            let measures = trace.borrow().measured.iter().filter(|(key, _)| *key == target).cloned().collect::<Vec<_>>();
            assert!(measures.len() >= 2, "live source validation rereads the same controller after projection");
            assert!(measures.iter().all(|(_, measured)| *measured == identity));
            if drift && incoming_owner {
                assert!(h.probe.state(&target).is_err(), "staged owner was never published");
            }
            assert!(std::sync::Arc::ptr_eq(&app.session.doc, &doc));
            assert!(std::sync::Arc::ptr_eq(&app.session.model.state(), &model));
            assert_eq!(app.session.revision, revision);
            assert_eq!(app.session.undo.len(), undo);
            assert_eq!(app.session.is_dirty(), dirty);
        }
    }
}

#[test]
fn home_visibility_transition_discards_shown_candidate_and_unmounts_without_stopping_retained_services() {
    for paint_home in [false, true] {
        let (mut h, probe, trace) = boundary_app();
        let key = app_editor_key(&h.app);
        probe.edit(&key, |state| state.draft = "Home retains this live draft".into()).unwrap();
        let scope = runtime_scope(h.app.panel_runtime().unwrap());
        h.app
            .panel_runtime_mut()
            .unwrap()
            .with_services(|_, services| {
                services.local.request(&scope, LocalRequest::KeyboardOwner { instance: id("editor"), control: ControlId::new("draft").unwrap() })
            })
            .unwrap();
        let header = app_control(&h.app, HostControlTarget::Pointer(HostPointerTarget::Header { instance: id("editor") }));
        let start = egui_rect(header.bounds).center();
        let frame = h.app.universal_panel_frame().unwrap();
        let landing = host_drop_targets(h.app.panel_runtime().unwrap().layout(), &frame.layout, &id("editor"))
            .unwrap()
            .into_iter()
            .find(|target| matches!(&target.id, HostDropTargetId::Dock { zone: Zone::Right, .. }) && target.rejection.is_none())
            .unwrap();
        let end = egui_rect(landing.bounds).center();
        h.frame_with(vec![egui::Event::PointerMoved(start), button_event(start, true)]);
        h.frame_with(vec![egui::Event::PointerMoved(end)]);
        let preview = h.app.universal_panel_frame().unwrap().preview.as_ref().unwrap();
        assert!(preview.action.is_some() && preview.projected.is_some() && preview.rejection.is_none());
        let before = h.app.panel_runtime().unwrap().layout().inspect();
        let count = h.app.panel_runtime().unwrap().controller_count();
        let state = probe.state(&key).unwrap();
        if paint_home {
            h.app.home.show(h.app.session.revision);
            h.frames(2); // lib.rs must observe Home even without ui.home.
        } else {
            set_home_without_paint(&mut h, true); // no intervening paint required
        }
        assert!(!h.app.panel_runtime_mut().unwrap().host().unwrap().interaction().is_captured());
        assert!(h.app.universal_panel_frame().is_none());
        assert!(!h.app.panel_runtime().unwrap().state(&key).unwrap().presented);
        assert!(!h.app.panel_runtime().unwrap().state(&key).unwrap().activated);
        assert!(trace.borrow().keyboard.is_empty(), "hidden owners release local keyboard input");
        let hidden = probe.state(&key).unwrap();
        h.frames(2);
        assert!(probe.state(&key).unwrap().updates > hidden.updates);
        assert_eq!(probe.state(&key).unwrap().renders, state.renders, "hidden bodies do not render while services tick");
        set_home_without_paint(&mut h, false);
        h.frames(2);
        h.frame_with(vec![egui::Event::PointerMoved(end), button_event(end, false)]);
        assert_eq!(h.app.panel_runtime().unwrap().layout().inspect(), before, "release cannot resurrect the old landing");
        assert_eq!(h.app.panel_runtime().unwrap().controller_count(), count);
        assert!(!h.app.panel_runtime_mut().unwrap().host().unwrap().interaction().is_captured());
        let retained = probe.state(&key).unwrap();
        assert_eq!(retained.draft, state.draft);
        assert_eq!(retained.cancels, state.cancels);
        assert_eq!(retained.destroys, state.destroys);
        assert!(retained.transaction_live);
    }
}

#[test]
fn home_open_close_between_paint_and_finish_discards_real_deferred_widget_intent() {
    let (mut h, probe, _) = boundary_app();
    let key = app_editor_key(&h.app);
    let collapse = app_control(&h.app, HostControlTarget::Pointer(HostPointerTarget::Collapse { instance: id("editor") }));
    let at = egui_rect(collapse.bounds).center();
    h.frame_with(vec![egui::Event::PointerMoved(at), button_event(at, true)]);
    let before = h.app.panel_runtime().unwrap().layout().inspect();
    let draft = probe.state(&key).unwrap();
    let layout = h.app.universal_panel_frame().unwrap().layout.clone();
    let time = h.ctx.input(|input| input.time) + 1.0 / 60.0;
    let mut output = h.ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0))),
            time: Some(time),
            focused: true,
            events: vec![button_event(at, false)],
            ..Default::default()
        },
        |ui| {
            h.app.logic(ui.ctx());
            let integration = h.app.panel_runtime.as_mut().unwrap();
            integration.prepare_host_in_domain(&h.app.session, layout.render_domain, layout.capacity, &layout.chrome_obstacles).unwrap();
            integration.show_host_deferred(&h.app.session, ui).unwrap();
            assert!(ui.ctx().read_response(collapse.id).unwrap().clicked(), "actual egui collapse click is pending");
            assert!(integration.host().unwrap().has_pending());
            for open in [true, false] {
                let (request, _) = crate::control::ControlRequest::new("ui.home", serde_json::json!({"open":open}));
                let _reply = crate::control::handle(&mut h.app, ui.ctx(), &request);
            }
            let integration = h.app.panel_runtime.as_mut().unwrap();
            assert!(!integration.host().unwrap().has_pending());
            integration.finish_host(&h.app.session, ui).unwrap();
        },
    );
    output.textures_delta.clear();
    assert_eq!(h.app.panel_runtime().unwrap().layout().inspect(), before);
    assert!(h.app.universal_panel_frame().is_none());
    h.frames(2);
    assert_eq!(h.app.panel_runtime().unwrap().layout().inspect(), before);
    assert_eq!(probe.state(&key).unwrap().draft, draft.draft);
    assert_eq!(probe.state(&key).unwrap().cancels, draft.cancels);
}

#[test]
fn wrapping_editor_keeps_keyboard_caret_and_identity_through_metric_and_reservation_invalidation() {
    for scale in [1.0, 2.0] {
        let (mut h, trace) = review_cohort_host(|_| {});
        h.dispatch(vec![fixture_open("fixture.editor", "wrapping-editor", "wrapping", Zone::Left, 0).unwrap()]).unwrap();
        let mut raw = RawPanelHost::new(h, scale);
        raw.frame(Vec::new(), false);
        raw.frame(Vec::new(), false);
        let editor = trace.borrow().editors.last().unwrap().clone();
        let at = editor.2.center();
        raw.pointer(at, true, false);
        raw.pointer(at, false, false);
        raw.ctx.memory_mut(|memory| memory.request_focus(editor.0));
        raw.frame(Vec::new(), false);
        let initial_rows = *trace.borrow().wrap_rows.last().unwrap();
        let before = raw.content.runtime.layout().inspect();
        let metric_revision = raw.host.frame().unwrap().layout.geometry_policy.content_revision;
        let (stale, _) = raw.frame_late(vec![egui::Event::Text(" more wrapped words".repeat(12))], false, |_, _| {});
        assert!(stale.is_none(), "new text invalidates the old metric geometry");
        assert_eq!(raw.ctx.memory(|memory| memory.focused()), Some(editor.0));
        assert!(*trace.borrow().wrap_rows.last().unwrap() > initial_rows, "the actual multiline editor now wraps");
        let caret = egui::TextEdit::load_state(&raw.ctx, editor.0).unwrap().cursor.char_range();
        let draft = trace.borrow().editors.last().unwrap().3.clone();
        raw.frame(Vec::new(), false);
        assert_ne!(raw.host.frame().unwrap().layout.geometry_policy.content_revision, metric_revision);
        assert_eq!(raw.ctx.memory(|memory| memory.focused()), Some(editor.0));
        assert_eq!(egui::TextEdit::load_state(&raw.ctx, editor.0).unwrap().cursor.char_range(), caret);
        assert_eq!(trace.borrow().editors.last().unwrap().0, editor.0);
        assert_eq!(trace.borrow().editors.last().unwrap().3, draft);
        let mut envelope = LayoutEnvelope::new(raw.content.runtime.layout().layout().clone());
        let mut pending = envelope.layout.instances[0].clone();
        pending.id = id("focus-extension");
        pending.kind = PanelKind::new("extension.focus").unwrap();
        pending.requested_open = false;
        pending.memory.section = section("focus-reserved-section");
        envelope.quarantine.push(QuarantinedPanel {
            instance: pending.id.clone(),
            kind: pending.kind.clone(),
            instance_data: serde_json::to_value(pending).unwrap(),
            section: None,
        });
        raw.content.catalogue.remember(&envelope, None, raw.content.runtime.layout().registry()).unwrap();
        raw.frame(Vec::new(), false);
        assert_eq!(raw.ctx.memory(|memory| memory.focused()), Some(editor.0), "reservation-only invalidation preserves eligible focus");
        assert_eq!(egui::TextEdit::load_state(&raw.ctx, editor.0).unwrap().cursor.char_range(), caret);
        let (stale, _) = raw.frame_late(vec![egui::Event::Text(" continued typing".into())], false, |_, _| {});
        assert!(stale.is_none());
        assert_eq!(raw.ctx.memory(|memory| memory.focused()), Some(editor.0));
        assert!(trace.borrow().editors.last().unwrap().3.contains("continued typing"));
        assert_eq!(raw.content.runtime.layout().inspect(), before);
        let saved_caret = egui::TextEdit::load_state(&raw.ctx, editor.0).unwrap().cursor.char_range();
        let mut environment = raw.content.runtime.environment().clone();
        environment.document = Some(DocumentId::new("focus-next-document").unwrap());
        raw.content.runtime.set_environment(environment, &mut raw.content.local).unwrap();
        raw.frame(Vec::new(), false);
        assert_ne!(raw.ctx.memory(|memory| memory.focused()), Some(editor.0), "owner change still releases keyboard input");
        assert_eq!(egui::TextEdit::load_state(&raw.ctx, editor.0).unwrap().cursor.char_range(), saved_caret);
        assert_eq!(raw.content.runtime.controller_count(), 2, "old owner draft/controller remain retained");
    }
}

fn set_home_without_paint(h: &mut crate::scenario::Harness, open: bool) {
    let (request, _) = crate::control::ControlRequest::new("ui.home", serde_json::json!({"open":open}));
    match crate::control::handle(&mut h.app, &h.ctx, &request) {
        crate::control::Outcome::Done(value) => assert_eq!(value["ok"], true),
        _ => panic!("Home must complete synchronously without a screenshot"),
    }
}
fn review_wheel(at: egui::Pos2, delta: f32) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(at),
        egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, delta),
            modifiers: egui::Modifiers::NONE,
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Unknown,
        },
    ]
}

#[test]
fn raw_clipped_header_body_surface_keeps_wheel_fallback_and_real_child_consumption_wins() {
    for scale in [1.0, 2.0] {
        let (mut h, trace) = review_cohort_host(|spec| {
            spec.min_size.height = 200.0;
            spec.default_size.height = 240.0;
        });
        h.dispatch(
            (0..3)
                .map(|index| fixture_open("fixture.log", &format!("wheel-{index}"), &format!("wheel-{index}"), Zone::Left, index).unwrap())
                .collect(),
        )
        .unwrap();
        let mut raw = RawPanelHost::new(h, scale);
        raw.capacity.size = Size { width: 600.0, height: 260.0 };
        raw.frame(Vec::new(), false);
        raw.frame(Vec::new(), false);
        let first = raw.host.frame().unwrap().layout.sections[0].clone();
        let initial_body = egui_rect(first.body.expect("expanded first card has a real body")).intersect(egui_rect(raw.capacity));
        let shift = first.header.origin.y + first.header.size.height - raw.capacity.origin.y + 2.0;
        raw.frame(review_wheel(initial_body.center(), -shift), false);
        for _ in 0..8 {
            raw.frame(Vec::new(), false);
        }
        let frame = raw.host.frame().unwrap();
        let first = frame.layout.sections.iter().find(|card| card.instance == id("wheel-0")).unwrap();
        assert!(first.header.origin.y + first.header.size.height <= raw.capacity.origin.y);
        assert!(
            !frame.controls.iter().any(|control| control.target == HostControlTarget::Pointer(HostPointerTarget::Header { instance: id("wheel-0") })),
            "the header response really is clipped away"
        );
        let body = egui_rect(first.body.unwrap()).intersect(egui_rect(raw.capacity));
        assert!(body.is_positive());
        let surface = raw_control(&raw, &HostControlTarget::Pointer(HostPointerTarget::Surface { instance: id("wheel-0") }));
        assert!(raw.ctx.read_response(surface.id).unwrap().interact_rect.contains(body.center()), "visible body has an actual current card response");
        let before = frame.layout.geometry_policy.scroll.y;
        raw.frame(review_wheel(body.center(), -12.0), false);
        raw.frame(Vec::new(), false);
        assert!(raw.host.frame().unwrap().layout.geometry_policy.scroll.y > before, "body wheel must work with no Header control");
        for _ in 0..8 {
            raw.frame(Vec::new(), false);
        }
        let frame = raw.host.frame().unwrap();
        let first = frame.layout.sections.iter().find(|card| card.instance == id("wheel-0")).unwrap();
        let body = egui_rect(first.body.expect("first body remains mounted after host scrolling")).intersect(egui_rect(raw.capacity));
        assert!(body.is_positive(), "host scrolling must leave a visible body for the child wheel fixture");
        let surface = raw_control(&raw, &HostControlTarget::Pointer(HostPointerTarget::Surface { instance: id("wheel-0") }));
        assert!(raw.ctx.read_response(surface.id).unwrap().interact_rect.contains(body.center()), "the clipped body still has an actual response");
        trace.borrow_mut().consume_wheel = true;
        raw.frame(Vec::new(), false);
        raw.frame(Vec::new(), false);
        let child = trace.borrow().child_scroll[&id("wheel-0")];
        assert!(child.0.is_positive());
        let host_offset = raw.host.frame().unwrap().layout.geometry_policy.scroll.y;
        raw.frame(review_wheel(child.0.center(), -12.0), false);
        raw.frame(Vec::new(), false);
        assert!(trace.borrow().child_scroll[&id("wheel-0")].1 > child.1, "the real inner ScrollArea consumed the wheel");
        assert!(
            (raw.host.frame().unwrap().layout.geometry_policy.scroll.y - host_offset).abs() < 0.001,
            "consumed child input cannot also scroll the host"
        );
    }
}

#[test]
fn uncovered_content_gap_routes_actual_wheel_to_the_existing_camera_path_at_both_scales() {
    for scale in [1.0, 2.0] {
        let (mut h, _, _) = boundary_app();
        h.ctx.set_pixels_per_point(scale);
        h.frames(2);
        let frame = h.app.universal_panel_frame().unwrap();
        let card = &frame.layout.sections[0];
        let at = egui::pos2(card.outer.origin.x + card.outer.size.width / 2.0, card.outer.origin.y + card.outer.size.height + 32.0);
        let point = Point { x: at.x, y: at.y };
        assert!(frame.layout.capacity.contains(point));
        assert!(!frame.layout.input.iter().any(|bounds| bounds.contains(point)));
        assert!(frame.layout.usable.contains_rect(LogicalRect { origin: point, size: Size { width: 2.0, height: 2.0 } }));
        let before = h.app.panel_runtime().unwrap().layout().inspect();
        let camera = h.app.cam;
        h.frame_with(review_wheel(at, -72.0));
        h.frames(3);
        assert_ne!(h.app.cam, camera, "the existing navigation handler receives the uncovered gap wheel");
        assert_eq!(h.app.panel_runtime().unwrap().layout().inspect(), before);
    }
}

fn priority_host_options() -> HostOptions {
    HostOptions {
        metrics: HostMetrics {
            header_height: 24.0,
            footer_height: 8.0,
            control_extent: 24.0,
            sash_width: 4.0,
            inline_max_height: 88.0,
            ..HostMetrics::default()
        },
        content_header_height: 0.0,
        ..Default::default()
    }
}
fn priority_raw_stack(scale: f32, direction: Direction, zone: Zone, capacity_extent: f32) -> (RawPanelHost, ReviewTrace) {
    let (mut h, trace) = review_cohort_host(|spec| {
        spec.min_size = Size { width: 120.0, height: 120.0 };
        spec.default_size = Size { width: 280.0, height: 200.0 };
        spec.max_size = Size { width: MAX_LOGICAL, height: MAX_LOGICAL };
    });
    h.dispatch(
        (0..3).map(|index| fixture_open("fixture.log", &format!("priority-{index}"), &format!("priority-{index}"), zone, index).unwrap()).collect(),
    )
    .unwrap();
    // Imported/common-tier preferences deliberately do not simulate three
    // sequential SetSize commands, which would author three different ranks.
    let mut layout = h.runtime.layout().layout().clone();
    for section in &mut layout.sections {
        section.size = if zone == Zone::Bottom { Size { width: 200.0, height: 200.0 } } else { Size { width: 280.0, height: 200.0 } };
        section.preferred_extent = Some(200.0);
        section.extent_priority = 0;
        section.weight = 1.0;
    }
    for instance in &mut layout.instances {
        instance.memory.size = if zone == Zone::Bottom { Size { width: 200.0, height: 200.0 } } else { Size { width: 280.0, height: 200.0 } };
        instance.memory.preferred_extent = Some(200.0);
        instance.memory.extent_priority = 0;
        instance.memory.weight = 1.0;
    }
    h.apply_layout(layout, None).unwrap();
    let mut environment = h.runtime.environment().clone();
    environment.direction = direction;
    h.runtime.set_environment(environment, &mut h.local).unwrap();
    let mut raw = RawPanelHost::new(h, scale);
    raw.host = WorkspacePanelHost::new(priority_host_options()).unwrap();
    raw.capacity.size =
        if zone == Zone::Bottom { Size { width: capacity_extent, height: 800.0 } } else { Size { width: 1000.0, height: capacity_extent } };
    raw.frame(Vec::new(), false);
    raw.frame(Vec::new(), false);
    (raw, trace)
}
fn priority_axis(size: Size, zone: Zone) -> f32 {
    if zone == Zone::Bottom { size.width } else { size.height }
}
fn assert_priority_number(actual: f32, expected: f32) {
    assert!((actual - expected).abs() <= 0.002, "actual {actual}, expected {expected}");
}
fn assert_priority_geometry(raw: &RawPanelHost, zone: Zone, direction: Direction, extents: [f32; 3]) {
    let host = &raw.host.frame().unwrap().layout;
    let mut offset = 0.0;
    for (index, expected) in extents.into_iter().enumerate() {
        let card = host.sections.iter().find(|card| card.section == section(&format!("priority-{index}"))).unwrap();
        let logical_origin = if zone != Zone::Bottom {
            card.outer.origin.y - host.capacity.origin.y
        } else if direction == Direction::Ltr {
            card.outer.origin.x - host.capacity.origin.x
        } else {
            host.capacity.origin.x + host.capacity.size.width - card.outer.origin.x - card.outer.size.width
        };
        assert_priority_number(priority_axis(card.outer.size, zone), expected);
        assert_priority_number(logical_origin, offset);
        offset += expected + 4.0;
    }
    assert_eq!(host.viewport, Some(host.render_domain));
    assert_eq!(host.extent, host.capacity.size);
}
fn raw_priority_pair_edit(raw: &mut RawPanelHost, first: usize, second: usize, zone: Zone, direction: Direction, wanted: f32) -> HostLayout {
    let first_id = section(&format!("priority-{first}"));
    let second_id = section(&format!("priority-{second}"));
    let before = raw.content.runtime.layout().inspect();
    let commits = raw.content.native.commits;
    let card = raw.host.frame().unwrap().layout.sections.iter().find(|card| card.section == first_id).unwrap();
    let delta = wanted - priority_axis(card.outer.size, zone);
    let at = raw.at(HostControlTarget::Pointer(HostPointerTarget::Sash { first: first_id, second: second_id }));
    raw.pointer(at, true, false);
    let end = if zone == Zone::Bottom {
        at + egui::vec2(if direction == Direction::Rtl { -delta } else { delta }, 0.0)
    } else {
        at + egui::vec2(0.0, delta)
    };
    let preview = raw.frame(vec![egui::Event::PointerMoved(end)], false).0.preview.expect("actual sash movement has a candidate");
    assert!(preview.rejection.is_none(), "{preview:?}");
    let shown = preview.projected.unwrap();
    assert_eq!(raw.content.runtime.layout().inspect(), before);
    let release = raw.pointer(end, false, false);
    assert_eq!(release.events.iter().filter(|event| matches!(event, HostEvent::PreparedCommitted { .. })).count(), 1);
    assert!(!release.events.iter().any(|event| matches!(event, HostEvent::Rejected { .. })));
    assert_eq!(raw.content.runtime.layout().inspect().revision, before.revision + 1);
    assert_eq!(raw.content.native.commits, commits + 1);
    raw.frame(Vec::new(), false);
    shown
}

#[test]
fn raw_underfilled_explicit_pair_edits_keep_real_gap_and_newest_overlapping_tier_on_compression() {
    for scale in [1.0, 2.0] {
        for direction in [Direction::Ltr, Direction::Rtl] {
            for zone in [Zone::Left, Zone::Bottom] {
                let (mut raw, trace) = priority_raw_stack(scale, direction, zone, 1000.0);
                let factories = trace.borrow().factories.clone();
                assert_priority_geometry(&raw, zone, direction, [200.0, 200.0, 200.0]);
                assert!(raw.content.runtime.layout().layout().sections.iter().all(|section| section.extent_priority == 0));
                let third = raw.host.frame().unwrap().layout.sections.iter().find(|card| card.section == section("priority-2")).unwrap().outer;
                let shown = raw_priority_pair_edit(&mut raw, 0, 1, zone, direction, 180.0);
                assert_priority_number(priority_axis(shown.sections[0].outer.size, zone), 180.0);
                assert_priority_geometry(&raw, zone, direction, [180.0, 220.0, 200.0]);
                assert_eq!(raw.host.frame().unwrap().layout.sections.iter().find(|card| card.section == section("priority-2")).unwrap().outer, third);
                let first = raw.content.runtime.layout().layout().section(&section("priority-0")).unwrap().clone();
                let second = raw.content.runtime.layout().layout().section(&section("priority-1")).unwrap();
                assert_eq!((first.preferred_extent, second.preferred_extent), (Some(180.0), Some(220.0)));
                assert!(first.extent_priority > 0);
                assert_eq!(first.extent_priority, second.extent_priority);
                assert_eq!(raw.content.runtime.layout().layout().section(&section("priority-2")).unwrap().preferred_extent, Some(200.0));
                assert_eq!(raw.content.runtime.layout().layout().section(&section("priority-2")).unwrap().extent_priority, 0);
                raw_priority_pair_edit(&mut raw, 1, 2, zone, direction, 190.0);
                assert_priority_geometry(&raw, zone, direction, [180.0, 190.0, 230.0]);
                let model = raw.content.runtime.layout().layout();
                let a = model.section(&section("priority-0")).unwrap();
                let b = model.section(&section("priority-1")).unwrap();
                let c = model.section(&section("priority-2")).unwrap();
                assert_eq!((a.preferred_extent, a.extent_priority), (first.preferred_extent, first.extent_priority));
                assert_eq!((b.preferred_extent, c.preferred_extent), (Some(190.0), Some(230.0)));
                assert!(b.extent_priority > a.extent_priority);
                assert_eq!(b.extent_priority, c.extent_priority);
                let saved = model.clone();
                let last = raw.host.frame().unwrap().layout.sections.iter().find(|card| card.section == section("priority-2")).unwrap().outer;
                let gap = if zone != Zone::Bottom {
                    Point { x: last.origin.x + 16.0, y: last.origin.y + last.size.height + 16.0 }
                } else if direction == Direction::Ltr {
                    Point { x: last.origin.x + last.size.width + 16.0, y: last.origin.y + 16.0 }
                } else {
                    Point { x: last.origin.x - 16.0, y: last.origin.y + 16.0 }
                };
                let used = if zone != Zone::Bottom {
                    last.origin.y + last.size.height - raw.capacity.origin.y
                } else if direction == Direction::Ltr {
                    last.origin.x + last.size.width - raw.capacity.origin.x
                } else {
                    raw.capacity.origin.x + raw.capacity.size.width - last.origin.x
                };
                assert_priority_number(priority_axis(raw.capacity.size, zone) - used, 392.0);
                assert!(raw.host.frame().unwrap().layout.usable.contains_rect(LogicalRect { origin: gap, size: Size { width: 4.0, height: 4.0 } }));
                assert!(!raw.host.frame().unwrap().layout.input.iter().any(|bounds| bounds.contains(gap)));
                if zone == Zone::Bottom {
                    raw.capacity.size.width = 400.0;
                } else {
                    raw.capacity.size.height = 400.0;
                }
                raw.frame(Vec::new(), false);
                assert_priority_geometry(&raw, zone, direction, [120.0, 120.0 + 112.0 / 9.0, 120.0 + 176.0 / 9.0]);
                assert_eq!(raw.content.runtime.layout().layout(), &saved, "compression never rewrites authored intent");
                if zone == Zone::Bottom {
                    raw.capacity.size.width = 1000.0;
                } else {
                    raw.capacity.size.height = 1000.0;
                }
                raw.frame(Vec::new(), false);
                assert_priority_geometry(&raw, zone, direction, [180.0, 190.0, 230.0]);
                assert_eq!(raw.content.runtime.layout().layout(), &saved);
                assert_eq!(trace.borrow().factories, factories);
            }
        }
    }
}

#[test]
fn raw_compressed_common_tier_pair_reaches_minimum_and_overlapping_edit_preserves_third() {
    for scale in [1.0, 2.0] {
        for direction in [Direction::Ltr, Direction::Rtl] {
            for zone in [Zone::Left, Zone::Bottom] {
                let (mut raw, trace) = priority_raw_stack(scale, direction, zone, 400.0);
                let factories = trace.borrow().factories.clone();
                assert_priority_geometry(&raw, zone, direction, [392.0 / 3.0; 3]);
                let third = raw.host.frame().unwrap().layout.sections.iter().find(|card| card.section == section("priority-2")).unwrap().outer;
                raw_priority_pair_edit(&mut raw, 0, 1, zone, direction, 120.0);
                assert_priority_geometry(&raw, zone, direction, [120.0, 424.0 / 3.0, 392.0 / 3.0]);
                let third_after = raw.host.frame().unwrap().layout.sections.iter().find(|card| card.section == section("priority-2")).unwrap().outer;
                assert_priority_number(priority_axis(third_after.size, zone), priority_axis(third.size, zone));
                assert_priority_number(
                    if zone == Zone::Bottom { third_after.origin.x } else { third_after.origin.y },
                    if zone == Zone::Bottom { third.origin.x } else { third.origin.y },
                );
                let first = raw.content.runtime.layout().layout().section(&section("priority-0")).unwrap().clone();
                assert_eq!(first.preferred_extent, Some(120.0));
                assert!(first.extent_priority > 0);
                let b = raw.content.runtime.layout().layout().section(&section("priority-1")).unwrap();
                assert_priority_number(b.preferred_extent.unwrap(), 424.0 / 3.0);
                assert_eq!(b.extent_priority, first.extent_priority);
                raw_priority_pair_edit(&mut raw, 1, 2, zone, direction, 132.0);
                assert_priority_geometry(&raw, zone, direction, [120.0, 132.0, 140.0]);
                let saved = raw.content.runtime.layout().layout().clone();
                assert_eq!((saved.sections[0].preferred_extent, saved.sections[0].extent_priority), (Some(120.0), first.extent_priority));
                assert_priority_number(saved.sections[1].preferred_extent.unwrap(), 132.0);
                assert_priority_number(saved.sections[2].preferred_extent.unwrap(), 140.0);
                assert!(saved.sections[1].extent_priority > first.extent_priority);
                assert_eq!(saved.sections[1].extent_priority, saved.sections[2].extent_priority);
                if zone == Zone::Bottom {
                    raw.capacity.size.width = 390.0;
                } else {
                    raw.capacity.size.height = 390.0;
                }
                raw.frame(Vec::new(), false);
                assert_priority_geometry(&raw, zone, direction, [120.0, 128.25, 133.75]);
                assert_eq!(raw.content.runtime.layout().layout(), &saved);
                assert_eq!(trace.borrow().factories, factories);
            }
        }
    }
}

fn priority_bottom_raw(
    scale: f32,
    direction: Direction,
    count: usize,
    authored_width: bool,
    unequal_height: bool,
    capacity_height: f32,
) -> (RawPanelHost, ReviewTrace) {
    let (mut h, trace) = review_cohort_host(|spec| {
        spec.min_size = Size { width: 120.0, height: 32.0 };
        spec.default_size = Size { width: 280.0, height: 64.0 };
        spec.max_size = Size { width: MAX_LOGICAL, height: 300.0 };
    });
    h.dispatch(
        (0..count)
            .map(|index| fixture_open("fixture.log", &format!("bottom-{index}"), &format!("bottom-{index}"), Zone::Bottom, index).unwrap())
            .collect(),
    )
    .unwrap();
    let mut layout = h.runtime.layout().layout().clone();
    let allocated_width = (1000.0 - 4.0 * (count - 1) as f32) / count as f32;
    for (index, section) in layout.sections.iter_mut().enumerate() {
        section.size = Size { width: 280.0, height: if unequal_height && index == 1 { 180.0 } else { 64.0 } };
        section.preferred_extent = authored_width.then_some(allocated_width);
        section.extent_priority = if authored_width { 7 } else { 0 };
        section.weight = 1.0;
    }
    for (index, instance) in layout.instances.iter_mut().enumerate() {
        instance.memory.size = layout.sections[index].size;
        instance.memory.preferred_extent = layout.sections[index].preferred_extent;
        instance.memory.extent_priority = layout.sections[index].extent_priority;
        instance.memory.weight = 1.0;
    }
    h.apply_layout(layout, None).unwrap();
    let mut environment = h.runtime.environment().clone();
    environment.direction = direction;
    h.runtime.set_environment(environment, &mut h.local).unwrap();
    let mut raw = RawPanelHost::new(h, scale);
    raw.host = WorkspacePanelHost::new(priority_host_options()).unwrap();
    raw.capacity.size = Size { width: 1000.0, height: capacity_height };
    raw.frame(Vec::new(), false);
    raw.frame(Vec::new(), false);
    (raw, trace)
}

#[test]
fn raw_bottom_only_exact_64_row_has_no_separator_and_passes_exact_capacity_admission() {
    for scale in [1.0, 2.0] {
        for direction in [Direction::Ltr, Direction::Rtl] {
            for unavailable_side in [false, true] {
                let (mut raw, trace) = priority_bottom_raw(scale, direction, 1, false, false, 64.0);
                if unavailable_side {
                    raw.content.dispatch(vec![fixture_open("fixture.log", "unavailable-side", "unavailable-side", Zone::Left, 1).unwrap()]).unwrap();
                    trace.borrow_mut().unavailable.insert(ControllerKey {
                        instance: id("unavailable-side"),
                        owner: ContentOwner::Workspace(raw.content.runtime.environment().workspace.clone()),
                    });
                    raw.frame(Vec::new(), false);
                }
                let host = &raw.host.frame().unwrap().layout;
                assert_eq!(host.sections.len(), 1);
                let card = &host.sections[0];
                assert_eq!(card.outer, LogicalRect { origin: Point { x: 0.0, y: 0.0 }, size: Size { width: 1000.0, height: 64.0 } });
                assert!(card.inline && card.body.is_none() && card.footer.is_none());
                assert_eq!(card.header, card.outer);
                assert_eq!(host.extent.height, 64.0);
                assert_eq!(host.viewport, Some(raw.capacity));
                let before = raw.content.runtime.layout().inspect();
                let action = transaction(vec![Mutation::SetBottomHeight { section: section("bottom-0"), height: 64.0 }]);
                let _candidate = raw
                    .content
                    .runtime
                    .prepare_host_proposal(host.clone(), action, &raw.content.models)
                    .expect("exact Bottom capacity admits without a phantom separator");
                assert_eq!(raw.content.runtime.layout().inspect(), before);
            }
        }
    }
}

#[test]
fn raw_bottom_thickness_drag_preserves_saved_width_preference_rank_and_shared_row() {
    for scale in [1.0, 2.0] {
        for direction in [Direction::Ltr, Direction::Rtl] {
            for count in [1, 2] {
                for authored_width in [false, true] {
                    for unequal_height in [false, true].into_iter().filter(|unequal| count == 2 || !*unequal) {
                        let (mut raw, trace) = priority_bottom_raw(scale, direction, count, authored_width, unequal_height, 800.0);
                        let before = raw.content.runtime.layout().layout().clone();
                        let factories = trace.borrow().factories.clone();
                        let old_geometry = raw.host.frame().unwrap().layout.clone();
                        let old_height = if unequal_height { 180.0 } else { 64.0 };
                        for card in &old_geometry.sections {
                            assert_eq!(card.outer.size.height, old_height);
                            assert_eq!(card.inline, !unequal_height, "64-point row is inline; a saved 180-point row is expanded");
                        }
                        let target =
                            HostControlTarget::Pointer(HostPointerTarget::Resize { section: section("bottom-0"), edge: HostResizeEdge::Top });
                        let at = raw.at(target);
                        raw.pointer(at, true, false);
                        let end = at + egui::vec2(0.0, old_height - 100.0);
                        let preview = raw.frame(vec![egui::Event::PointerMoved(end)], false).0.preview.expect("actual top-edge drag has a candidate");
                        assert!(preview.rejection.is_none(), "{preview:?}");
                        assert!(
                            matches!(one_mutation(preview.action.as_ref().unwrap()), Mutation::SetBottomHeight { section: id, height } if *id == section("bottom-0") && *height == 100.0)
                        );
                        let shown = preview.projected.unwrap();
                        for card in &shown.sections {
                            let previous = old_geometry.sections.iter().find(|old| old.section == card.section).unwrap();
                            assert_eq!(card.outer.size, Size { width: if count == 1 { 1000.0 } else { 498.0 }, height: 100.0 });
                            assert_eq!(card.outer.origin.y, 700.0);
                            assert_eq!(card.outer.origin.x, previous.outer.origin.x);
                        }
                        let release = raw.pointer(end, false, false);
                        assert_eq!(release.events.iter().filter(|event| matches!(event, HostEvent::PreparedCommitted { .. })).count(), 1);
                        raw.frame(Vec::new(), false);
                        let actual = &raw.host.frame().unwrap().layout;
                        assert_eq!(actual.render_domain, old_geometry.render_domain);
                        assert_eq!(actual.viewport, old_geometry.viewport);
                        assert_eq!(actual.extent, old_geometry.extent);
                        for card in &actual.sections {
                            let previous = old_geometry.sections.iter().find(|old| old.section == card.section).unwrap();
                            assert_eq!(card.outer.origin, Point { x: previous.outer.origin.x, y: 700.0 });
                            assert_eq!(card.outer.size, Size { width: if count == 1 { 1000.0 } else { 498.0 }, height: 100.0 });
                            assert!(!card.inline && card.body.is_some(), "crossing 88 uses the expanded composition");
                        }
                        let model = raw.content.runtime.layout().layout();
                        for section in &model.sections {
                            let prior = before.section(&section.id).unwrap();
                            assert_eq!(
                                (section.size.width, section.preferred_extent, section.extent_priority, section.weight),
                                (prior.size.width, prior.preferred_extent, prior.extent_priority, prior.weight)
                            );
                            assert_eq!(section.size.width, 280.0);
                            assert_eq!(section.size.height, 100.0);
                        }
                        for instance in &model.instances {
                            let prior = before.instance(&instance.id).unwrap();
                            assert_eq!(
                                (
                                    instance.memory.size.width,
                                    instance.memory.preferred_extent,
                                    instance.memory.extent_priority,
                                    instance.memory.weight
                                ),
                                (prior.memory.size.width, prior.memory.preferred_extent, prior.memory.extent_priority, prior.memory.weight)
                            );
                            assert_eq!(instance.memory.size.height, 100.0);
                        }
                        assert_eq!(trace.borrow().factories, factories);
                    }
                }
            }
        }
    }
}

#[test]
fn raw_bottom_thickness_release_without_move_cannot_commit_an_unseen_landing() {
    for scale in [1.0, 2.0] {
        for direction in [Direction::Ltr, Direction::Rtl] {
            let (mut raw, _) = priority_bottom_raw(scale, direction, 1, false, false, 800.0);
            let before = raw.content.runtime.layout().inspect();
            let commits = raw.content.native.commits;
            let at = raw.at(HostControlTarget::Pointer(HostPointerTarget::Resize { section: section("bottom-0"), edge: HostResizeEdge::Top }));
            raw.pointer(at, true, false);
            raw.frame(vec![button_event(at - egui::vec2(0.0, 36.0), false)], false);
            assert_eq!(raw.content.runtime.layout().inspect(), before);
            assert_eq!(raw.content.native.commits, commits);
            assert!(!raw.host.interaction().is_captured());
            assert!(raw.host.frame().unwrap().preview.is_none());
        }
    }
}

#[test]
fn atomic_overlapping_explicit_pair_commands_keep_composed_tiers_and_one_cohort() {
    for direction in [Direction::Ltr, Direction::Rtl] {
        let (mut raw, trace) = priority_raw_stack(1.0, direction, Zone::Left, 1000.0);
        let factories = trace.borrow().factories.clone();
        let commits = raw.content.native.commits;
        let registry = raw.content.runtime.layout().registry().clone();
        let h = &mut raw.content;
        {
            let mut services = ContentServices { models: &h.models, commands: &mut h.commands, local: &mut h.local };
            let mut live = RuntimeActionSink { runtime: &mut h.runtime, native: &mut h.native, services: &mut services };
            let mut sink = super::app::AdmittedActionSink { live: &mut live, host: Some(&mut raw.host), proposal: std::cell::RefCell::new(None) };
            commands::execute_batch(
                &[
                    serde_json::json!({"command":"panel.resize_pair","params":{"first":"priority-0","second":"priority-1",
                    "first_size":{"width":280,"height":180},"second_size":{"width":280,"height":220}}}),
                    serde_json::json!({"command":"panel.resize_pair","params":{"first":"priority-1","second":"priority-2",
                    "first_size":{"width":280,"height":190},"second_size":{"width":280,"height":230}}}),
                ],
                &mut commands::PanelCommandContext { sink: &mut sink, registry: &registry, catalogue: &mut h.catalogue, web: false },
            )
            .unwrap();
        }
        raw.frame(Vec::new(), false);
        assert_priority_geometry(&raw, Zone::Left, direction, [180.0, 190.0, 230.0]);
        let model = raw.content.runtime.layout().layout();
        assert!(model.sections[0].extent_priority > 0);
        assert!(model.sections[1].extent_priority > model.sections[0].extent_priority);
        assert_eq!(model.sections[1].extent_priority, model.sections[2].extent_priority);
        assert_eq!(raw.content.native.commits, commits + 1);
        assert_eq!(trace.borrow().factories, factories);
    }
}

#[test]
fn deferred_outgoing_unavailable_actions_use_captured_retained_or_staged_owner_availability() {
    use commands::LayoutCatalogue;
    for staged in [false, true] {
        for operation in ["open", "focus", "move"] {
            for available in [false, true] {
                let (mut h, trace) = review_cohort_host_with_layout(
                    |_| {},
                    |service| {
                        apply_fixture(service, vec![fixture_open("fixture.log", "deferred", "deferred", Zone::Left, 0).unwrap()]);
                        if operation == "open" {
                            apply_fixture(service, vec![Mutation::Close { instance: id("deferred") }]);
                        }
                        service
                            .set_availability([(id("deferred"), Availability::Unavailable { reason: "outgoing owner unavailable".into() })].into())
                            .unwrap();
                    },
                );
                let key = ControllerKey { instance: id("deferred"), owner: ContentOwner::Workspace(h.runtime.environment().workspace.clone()) };
                if !staged {
                    trace.borrow_mut().unavailable.insert(key.clone());
                    h.tick().unwrap();
                }
                if available {
                    trace.borrow_mut().unavailable.remove(&key);
                } else {
                    trace.borrow_mut().unavailable.insert(key.clone());
                }
                let mut host = WorkspacePanelHost::new(priority_host_options()).unwrap();
                host.capture_metrics(&h.runtime, &h.models).unwrap();
                host.capture_reservations(&h.runtime, &h.catalogue).unwrap();
                host.prepare(&h.runtime, capacity()).unwrap();
                assert_ne!(h.runtime.layout().availability(&id("deferred")), Availability::Available);
                let before = h.runtime.layout().inspect();
                let controllers = h.runtime.controller_count();
                let catalogue = h.catalogue.state().clone();
                let commits = h.native.commits;
                let factories = trace.borrow().factories.len();
                let measures = trace.borrow().measures.len();
                let reads = trace.borrow().availability_reads;
                let mutation = match operation {
                    "open" => Mutation::Open { kind: PanelKind::new("fixture.log").unwrap(), instance: id("deferred"), destination: None },
                    "focus" => Mutation::Focus { instance: id("deferred") },
                    "move" => Mutation::Move {
                        instance: id("deferred"),
                        destination: Destination::NewSection {
                            section: section("deferred"),
                            index: 0,
                            location: SectionLocation::Dock { zone: Zone::Right },
                            size: Size { width: 280.0, height: 240.0 },
                        },
                    },
                    other => panic!("unknown deferred operation fixture: {other}"),
                };
                let action = LayoutAction::Apply { expected_revision: Some(before.revision), mutations: vec![mutation] };
                let registry = h.runtime.layout().registry().clone();
                {
                    let mut services = ContentServices { models: &h.models, commands: &mut h.commands, local: &mut h.local };
                    let mut live = RuntimeActionSink { runtime: &mut h.runtime, native: &mut h.native, services: &mut services };
                    let mut sink = super::app::AdmittedActionSink { live: &mut live, host: Some(&mut host), proposal: std::cell::RefCell::new(None) };
                    if available {
                        let projected = sink.project(action.clone()).expect("deferred projection must reach the fresh proposed owner");
                        assert_eq!(sink.project(action.clone()).unwrap(), projected);
                        assert_eq!(sink.inspect(), before, "projection cannot publish captured availability or layout");
                        assert_eq!(sink.live.runtime.controller_count(), controllers, "staged controllers remain private before dispatch");
                        assert_eq!(trace.borrow().factories.len(), factories + usize::from(staged));
                        assert_eq!(trace.borrow().measures.len(), measures + 1, "one immutable provider captured for this proposal");
                        commands::execute_layout_action(
                            action,
                            &mut commands::PanelCommandContext { sink: &mut sink, registry: &registry, catalogue: &mut h.catalogue, web: false },
                        )
                        .unwrap();
                    } else {
                        assert_eq!(sink.project(action).unwrap_err().rule, Rule::Availability);
                        assert!(sink.proposal.borrow().is_none());
                    }
                }
                assert!(trace.borrow().availability_reads > reads, "the proposed-owner capture runs before final availability refusal");
                assert_eq!(trace.borrow().factories.len(), factories + usize::from(staged), "no factory replay between projection and dispatch");
                if available {
                    assert_eq!(h.runtime.layout().availability(&id("deferred")), Availability::Available);
                    assert!(h.runtime.layout().layout().instance(&id("deferred")).unwrap().requested_open);
                    assert_eq!(h.runtime.controller_count(), controllers + usize::from(staged));
                    assert_eq!(h.native.commits, commits + 1);
                    if operation == "move" {
                        assert_eq!(h.runtime.layout().layout().containing(&id("deferred")).unwrap().location.zone(), Zone::Right);
                    }
                    if operation == "focus" {
                        assert_eq!(h.runtime.layout().interaction().focused, Some(id("deferred")));
                    }
                } else {
                    assert_eq!(h.runtime.layout().inspect(), before);
                    assert_eq!(h.runtime.controller_count(), controllers);
                    assert_eq!(h.catalogue.state(), &catalogue);
                    assert_eq!(h.native.commits, commits);
                    if staged {
                        assert!(h.probe.state(&key).is_err(), "unavailable staged controller was never published");
                    }
                }
                assert_eq!(h.commands.calls, 0);
            }
        }
    }
}

#[test]
fn raw_bottom_tab_header_top_edge_resize_owns_label_grip_and_close_overlaps() {
    for scale in [1.0, 2.0] {
        for direction in [Direction::Ltr, Direction::Rtl] {
            let (mut content, trace) = review_cohort_host(|_| {});
            content
                .dispatch(vec![
                    fixture_open("fixture.editor", "editor", "bottom-tabs", Zone::Bottom, 0).unwrap(),
                    Mutation::Open {
                        kind: PanelKind::new("fixture.log").unwrap(),
                        instance: id("inactive"),
                        destination: Some(Destination::JoinTab { section: section("bottom-tabs"), index: 1 }),
                    },
                    Mutation::Activate { instance: id("editor") },
                ])
                .unwrap();
            let mut environment = content.runtime.environment().clone();
            environment.direction = direction;
            content.runtime.set_environment(environment.clone(), &mut content.local).unwrap();
            let key = editor_key(&content);
            content.probe.edit(&key, |state| state.draft = "resize retains this document draft".into()).unwrap();
            let PanelMountHarness { runtime, probe, catalogue, .. } = content;
            let mut session = solidcraft_engine::Session::default();
            session.document_id = "fixture.document".into();
            let mut app = crate::SolveApp::new(session, Default::default());
            app.enable_panel_runtime(runtime, Box::new(FakeAppPanelServices { models: FakeModels { editable: true }, ..Default::default() }))
                .unwrap();
            let native = std::rc::Rc::new(std::cell::RefCell::new(FakeNativeAdapter::new(false)));
            app.panel_runtime_mut().unwrap().set_command_adapters(Box::new(catalogue), Box::new(SharedNative(native.clone()))).unwrap();
            app.enable_universal_panel_host(priority_host_options()).unwrap();
            let ctx = crate::fonts::test_context();
            ctx.enable_accesskit();
            ctx.set_pixels_per_point(scale);
            let capacity = rect(0.0, 0.0, 800.0, 800.0);
            let mut time = 0.0;
            let mut frame = |app: &mut crate::SolveApp, events: Vec<egui::Event>| {
                time += 1.0 / 60.0;
                app.panel_runtime.as_mut().unwrap().prepare_host(&app.session, capacity).unwrap();
                let mut output = ctx.run_ui(
                    egui::RawInput { screen_rect: Some(egui_rect(capacity)), events, time: Some(time), focused: true, ..Default::default() },
                    |ui| {
                        app.panel_runtime.as_mut().unwrap().show_host(&app.session, ui).unwrap();
                    },
                );
                output.textures_delta.clear();
                app.universal_panel_frame().unwrap().clone()
            };
            frame(&mut app, Vec::new());
            let initial = frame(&mut app, Vec::new());
            assert_eq!(initial.layout.sections[0].outer, rect(0.0, 560.0, 800.0, 240.0));
            assert_eq!(initial.layout.sections[0].header.size.height, 24.0);
            let factories = trace.borrow().factories.clone();
            let retained = probe.state(&key).unwrap();
            let doc = app.session.doc.clone();
            let model = app.session.model.state();
            let camera = app.cam;
            let revision = app.session.revision;
            let undo = app.session.undo.len();
            let redo = app.session.redo.len();
            let dirty = app.session.is_dirty();
            // Reuse the same card for three gestures; no duplicate tab-centre
            // click/drag matrix. Those existing cases continue to cover tabs.
            for underlay in ["inactive-tab", "grip", "close"] {
                let before = app.panel_runtime().unwrap().layout().inspect();
                let commits = native.borrow().commits;
                let actual = app.universal_panel_frame().unwrap().layout.clone();
                let card = &actual.sections[0];
                let bounds = match underlay {
                    "inactive-tab" => card.tabs.iter().find(|tab| tab.instance == id("inactive")).unwrap().bounds,
                    "grip" => card.title.unwrap(),
                    _ => card.close.unwrap(),
                };
                let start = egui::pos2(middle(bounds).x, card.header.origin.y + 2.0);
                assert!(bounds.contains(Point { x: start.x, y: start.y }));
                let resize = HostPointerTarget::Resize { section: section("bottom-tabs"), edge: HostResizeEdge::Top };
                assert_eq!(actual.hit_test(Point { x: start.x, y: start.y }), Some(resize.clone()));
                let resize_id = app_control(&app, HostControlTarget::Pointer(resize)).id;
                frame(&mut app, vec![egui::Event::PointerMoved(start)]);
                frame(&mut app, vec![button_event(start, true)]);
                assert!(ctx.read_response(resize_id).unwrap().is_pointer_button_down_on(), "the actual sash owns {underlay}/{direction:?}/{scale}");
                assert!(app.panel_runtime.as_ref().unwrap().host().unwrap().interaction().is_captured());
                let end = start - egui::vec2(0.0, 40.0);
                let shown = frame(&mut app, vec![egui::Event::PointerMoved(end)]).preview.expect("top-edge resize starts above header controls");
                assert!(shown.rejection.is_none(), "{shown:?}");
                assert!(matches!(one_mutation(shown.action.as_ref().unwrap()), Mutation::SetBottomHeight { height, .. }
                    if *height == card.outer.size.height + 40.0));
                let projected = shown.projected.unwrap();
                assert_eq!(projected.sections[0].outer, rect(0.0, card.outer.origin.y - 40.0, 800.0, card.outer.size.height + 40.0));
                assert_eq!(app.panel_runtime().unwrap().layout().inspect(), before, "preview cannot mutate layout or select the covered tab");
                assert_eq!(app.universal_panel_frame().unwrap().layout.sections, actual.sections);
                let release = frame(&mut app, vec![button_event(end, false)]);
                assert_eq!(release.events.iter().filter(|event| matches!(event, HostEvent::PreparedCommitted { .. })).count(), 1);
                assert_eq!(native.borrow().commits, commits + 1);
                let landed = frame(&mut app, Vec::new());
                assert_host_landing(&landed.layout, &projected);
                let after = app.panel_runtime().unwrap().layout().inspect();
                assert_eq!(after.revision, before.revision + 1);
                assert_eq!(after.layout.sections[0].tabs, before.layout.sections[0].tabs);
                assert_eq!(after.layout.sections[0].active, id("editor"));
                let repeated = frame(&mut app, vec![button_event(end, false)]);
                assert!(!repeated.events.iter().any(|event| matches!(event, HostEvent::PreparedCommitted { .. })));
                assert_eq!(app.panel_runtime().unwrap().layout().inspect(), after);
                assert_eq!(native.borrow().commits, commits + 1);
                assert!(repeated.menu_bounds.is_none());
                assert_eq!(app.panel_runtime().unwrap().controller_count(), 2);
                assert_eq!(trace.borrow().factories, factories);
                assert!(trace.borrow().actions.is_empty());
                let state = probe.state(&key).unwrap();
                assert_eq!(state.draft, retained.draft);
                assert_eq!(
                    (state.cancels, state.reparents, state.reopens, state.destroys),
                    (retained.cancels, retained.reparents, retained.reopens, retained.destroys)
                );
                assert_eq!(app.panel_runtime().unwrap().environment(), &environment);
                assert!(std::sync::Arc::ptr_eq(&app.session.doc, &doc));
                assert!(std::sync::Arc::ptr_eq(&app.session.model.state(), &model));
                assert_eq!(app.cam, camera);
                assert_eq!(app.session.revision, revision);
                assert_eq!(app.session.is_dirty(), dirty);
                assert_eq!(app.session.undo.len(), undo);
                assert_eq!(app.session.redo.len(), redo);
            }
        }
    }
}

#[test]
fn raw_bottom_two_tab_expanded_120_drag_down_88_reaches_inline_32_without_changing_width_intent() {
    for scale in [1.0, 2.0] {
        for direction in [Direction::Ltr, Direction::Rtl] {
            let (mut h, trace) = review_cohort_host(|spec| {
                spec.min_size = Size { width: 120.0, height: 24.0 };
                spec.default_size = Size { width: 280.0, height: 120.0 };
                spec.max_size = Size { width: MAX_LOGICAL, height: 300.0 };
            });
            h.dispatch(vec![
                fixture_open("fixture.log", "bottom-active", "bottom-tabs", Zone::Bottom, 0).unwrap(),
                Mutation::Open {
                    kind: PanelKind::new("fixture.log").unwrap(),
                    instance: id("bottom-inactive"),
                    destination: Some(Destination::JoinTab { section: section("bottom-tabs"), index: 1 }),
                },
                Mutation::Activate { instance: id("bottom-active") },
            ])
            .unwrap();
            let mut layout = h.runtime.layout().layout().clone();
            layout.sections[0].size = Size { width: 280.0, height: 120.0 };
            layout.sections[0].preferred_extent = Some(1000.0);
            layout.sections[0].extent_priority = 7;
            layout.sections[0].weight = 0.75;
            for instance in &mut layout.instances {
                instance.memory.size = layout.sections[0].size;
                instance.memory.preferred_extent = Some(1000.0);
                instance.memory.extent_priority = 7;
                instance.memory.weight = 0.75;
            }
            h.apply_layout(layout, None).unwrap();
            let mut environment = h.runtime.environment().clone();
            environment.direction = direction;
            h.runtime.set_environment(environment, &mut h.local).unwrap();
            let mut options = priority_host_options();
            options.metrics.footer_height = 16.0;
            let mut raw = RawPanelHost::new(h, scale);
            raw.host = WorkspacePanelHost::new(options).unwrap();
            raw.capacity.size = Size { width: 1000.0, height: 800.0 };
            raw.frame(Vec::new(), false);
            raw.frame(Vec::new(), false);
            let before = raw.content.runtime.layout().inspect();
            let factories = trace.borrow().factories.clone();
            let old = raw.host.frame().unwrap().layout.sections[0].clone();
            assert_eq!(old.outer, LogicalRect { origin: Point { x: 0.0, y: 680.0 }, size: Size { width: 1000.0, height: 120.0 } });
            assert!(!old.inline);
            assert_eq!(old.header.size.height, 24.0);
            assert_eq!(old.tabs.len(), 2);
            assert!(old.tabs.iter().all(|tab| tab.bounds.size.height == old.header.size.height));
            assert_eq!(old.footer.unwrap().size.height, 16.0);
            assert!(old.body.is_some());
            let start = raw.at(HostControlTarget::Pointer(HostPointerTarget::Resize { section: section("bottom-tabs"), edge: HostResizeEdge::Top }));
            raw.pointer(start, true, false);
            let end = start + egui::vec2(0.0, 88.0);
            let shown =
                raw.frame(vec![egui::Event::PointerMoved(end)], false).0.preview.expect("actual top-edge drag must cross composition boundary");
            assert!(shown.rejection.is_none(), "{shown:?}");
            assert!(matches!(one_mutation(shown.action.as_ref().unwrap()), Mutation::SetBottomHeight { height, .. } if *height == 32.0));
            let candidate = &shown.projected.as_ref().unwrap().sections[0];
            assert_eq!(candidate.outer, LogicalRect { origin: Point { x: 0.0, y: 768.0 }, size: Size { width: 1000.0, height: 32.0 } });
            assert!(candidate.inline && candidate.body.is_none() && candidate.footer.is_none());
            assert_eq!(candidate.header, candidate.outer);
            assert_eq!(raw.content.runtime.layout().inspect(), before, "preview is observational");
            let release = raw.pointer(end, false, false);
            assert_eq!(release.events.iter().filter(|event| matches!(event, HostEvent::PreparedCommitted { .. })).count(), 1);
            raw.frame(Vec::new(), false);
            let card = &raw.host.frame().unwrap().layout.sections[0];
            assert_eq!(card.outer.origin.y, 768.0);
            assert_eq!(card.outer.size, Size { width: 1000.0, height: 32.0 });
            assert!(card.inline && card.inline_content.is_some() && card.body.is_none() && card.footer.is_none());
            let after = raw.content.runtime.layout().layout();
            let saved = &before.layout.sections[0];
            let section = &after.sections[0];
            assert_eq!(
                (section.size.width, section.preferred_extent, section.extent_priority, section.weight),
                (saved.size.width, saved.preferred_extent, saved.extent_priority, saved.weight)
            );
            assert_eq!(section.tabs, saved.tabs);
            assert_eq!(section.active, saved.active);
            assert_eq!(section.size.height, 32.0);
            for instance in &after.instances {
                let old = &before.layout.instance(&instance.id).unwrap().memory;
                assert_eq!(
                    (instance.memory.size.width, instance.memory.preferred_extent, instance.memory.extent_priority, instance.memory.weight),
                    (old.size.width, old.preferred_extent, old.extent_priority, old.weight)
                );
                assert_eq!(instance.memory.size.height, 32.0);
            }
            assert_eq!(trace.borrow().factories, factories);
            assert_eq!(raw.content.commands.calls, 0);
            // The additive registered request follows the same C/context route
            // back across the boundary. Its valid ignored width must not author
            // a Bottom width preference or a new extent tier.
            run_context_pair_batch(
                &mut raw,
                &[serde_json::json!({"command":"panel.resize","params":{
                "target":{"type":"section","id":"bottom-tabs"},"size":{"width":1000,"height":120},"thickness_only":true}})],
            )
            .unwrap();
            raw.frame(Vec::new(), false);
            let restored = &raw.host.frame().unwrap().layout.sections[0];
            assert!(!restored.inline && restored.body.is_some());
            assert_eq!(restored.outer.origin.y, 680.0);
            assert_eq!(restored.outer.size.height, 120.0);
            let saved = &raw.content.runtime.layout().layout().sections[0];
            assert_eq!((saved.size.width, saved.preferred_extent, saved.extent_priority, saved.weight), (280.0, Some(1000.0), 7, 0.75));
            assert_eq!(trace.borrow().factories, factories);
        }
    }
}

fn context_batch_raw(scale: f32, direction: Direction, c_open_floating: bool, captured_available: bool) -> (RawPanelHost, ReviewTrace) {
    let (mut h, trace) = review_cohort_host_with_layout(
        |spec| {
            spec.min_size = Size { width: 120.0, height: 120.0 };
            spec.default_size = Size { width: 280.0, height: 240.0 };
            spec.max_size = Size { width: MAX_LOGICAL, height: MAX_LOGICAL };
        },
        |service| {
            apply_fixture(
                service,
                vec![
                    fixture_open("fixture.log", "pair-a", "pair-a", Zone::Left, 0).unwrap(),
                    fixture_open("fixture.log", "pair-b", "pair-b", Zone::Left, 1).unwrap(),
                    Mutation::Open {
                        kind: PanelKind::new("fixture.application").unwrap(),
                        instance: id("pair-c"),
                        destination: Some(Destination::NewSection {
                            section: section("pair-c"),
                            index: 2,
                            location: SectionLocation::Floating { origin: Point { x: 600.0, y: 100.0 } },
                            size: Size { width: 280.0, height: 240.0 },
                        }),
                    },
                ],
            );
            if !c_open_floating {
                apply_fixture(service, vec![Mutation::Close { instance: id("pair-c") }]);
            }
        },
    );
    let mut environment = h.runtime.environment().clone();
    environment.direction = direction;
    h.runtime.set_environment(environment, &mut h.local).unwrap();
    let mut raw = RawPanelHost::new(h, scale);
    raw.host = WorkspacePanelHost::new(priority_host_options()).unwrap();
    raw.capacity.size = Size { width: 1000.0, height: 800.0 };
    raw.frame(Vec::new(), false);
    raw.frame(Vec::new(), false);
    assert_ne!(raw.content.runtime.layout().availability(&id("pair-c")), Availability::Available);
    assert_eq!(raw.content.runtime.controller_count(), 2, "C is honestly unbound in outgoing presentation");
    let binding = fixture_controller_binding(&raw.content.probe, false, Ownership::Workspace);
    let create = binding.create;
    let capture_trace = trace.clone();
    raw.content
        .runtime
        .register(
            PanelKind::new("fixture.application").unwrap(),
            ControllerBinding {
                ownership: Ownership::Workspace,
                create: Box::new(move |instance, owner| {
                    let key = ControllerKey { instance: instance.id.clone(), owner: owner.clone() };
                    let serial = capture_trace.borrow().factories.len() + 1;
                    let inner = create(instance, owner)?;
                    capture_trace.borrow_mut().factories.push((key.clone(), serial));
                    Ok(Box::new(ReviewController { inner, key, serial, trace: capture_trace.clone(), draft: "staged batch draft".into() }))
                }),
            },
        )
        .unwrap();
    if !captured_available {
        trace
            .borrow_mut()
            .unavailable
            .insert(ControllerKey { instance: id("pair-c"), owner: ContentOwner::Workspace(raw.content.runtime.environment().workspace.clone()) });
    }
    // No tick after binding: capture must obtain C's new answer through its one
    // private incoming object, while the real outgoing service stays unavailable.
    (raw, trace)
}
fn context_pair_step(first: f32, second: f32) -> serde_json::Value {
    serde_json::json!({"command":"panel.resize_pair","params":{"first":"pair-a","second":"pair-b",
        "first_size":{"width":280,"height":first},"second_size":{"width":280,"height":second}}})
}
fn context_c_step(open: bool, floating: bool) -> serde_json::Value {
    let location =
        if floating { serde_json::json!({"type":"floating","origin":{"x":600,"y":100}}) } else { serde_json::json!({"type":"dock","zone":"left"}) };
    let destination = serde_json::json!({"type":"new_section","section":"pair-c","index":2,"location":location,"size":{"width":280,"height":240}});
    if open {
        serde_json::json!({"command":"panel.open","params":{"kind":"fixture.application","instance":"pair-c","destination":destination}})
    } else {
        serde_json::json!({"command":"panel.move","params":{"target":{"type":"instance","id":"pair-c"},"destination":destination}})
    }
}
fn run_context_pair_batch(raw: &mut RawPanelHost, steps: &[serde_json::Value]) -> PanelResult<serde_json::Value> {
    let h = &mut raw.content;
    let registry = h.runtime.layout().registry().clone();
    let mut services = ContentServices { models: &h.models, commands: &mut h.commands, local: &mut h.local };
    let mut live = RuntimeActionSink { runtime: &mut h.runtime, native: &mut h.native, services: &mut services };
    let mut sink = super::app::AdmittedActionSink { live: &mut live, host: Some(&mut raw.host), proposal: std::cell::RefCell::new(None) };
    commands::execute_batch(
        steps,
        &mut commands::PanelCommandContext { sink: &mut sink, registry: &registry, catalogue: &mut h.catalogue, web: false },
    )
}
fn assert_context_batch_identity(raw: &RawPanelHost, trace: &ReviewTrace, before_factories: usize, before_providers: usize) {
    let trace = trace.borrow();
    assert_eq!(trace.factories.len(), before_factories + 1, "one staged C factory across every composed prefix");
    let serial = trace.factories.iter().find(|(key, _)| key.instance == id("pair-c")).unwrap().1;
    assert_eq!(trace.factories.iter().filter(|(key, _)| key.instance == id("pair-c")).count(), 1);
    assert_eq!(trace.providers.iter().filter(|(measured, _)| *measured == serial).count(), 1, "same C provider is retained while the batch grows");
    assert_eq!(trace.providers.len(), before_providers + 3, "one proposed provider for A, B and C across all prefixes");
    assert!(trace.lifecycle.iter().any(|(identity, event)| *identity == serial && matches!(event, LifecycleEvent::State(_))));
    assert_eq!(raw.content.runtime.controller_count(), 3);
    assert_eq!(raw.content.commands.calls, 0);
}

#[test]
fn raw_pair_batch_and_closed_outgoing_unavailable_floating_open_share_one_captured_cohort_in_either_order() {
    use commands::LayoutCatalogue;
    for scale in [1.0, 2.0] {
        for direction in [Direction::Ltr, Direction::Rtl] {
            for open_first in [false, true] {
                for available in [false, true] {
                    let (mut raw, trace) = context_batch_raw(scale, direction, false, available);
                    let before = raw.content.runtime.layout().inspect();
                    let catalogue = raw.content.catalogue.state().clone();
                    let commits = raw.content.native.commits;
                    let factories = trace.borrow().factories.len();
                    let providers = trace.borrow().providers.len();
                    let original = raw.host.frame().unwrap().layout.clone();
                    assert_eq!(original.sections.iter().map(|card| card.outer.size.height).collect::<Vec<_>>(), vec![398.0, 398.0]);
                    let pair = context_pair_step(180.0, 616.0);
                    let open = context_c_step(true, true);
                    let steps = if open_first { vec![open, pair] } else { vec![pair, open] };
                    let result = run_context_pair_batch(&mut raw, &steps);
                    if !available {
                        assert_eq!(result.unwrap_err().rule, Rule::Availability);
                        assert_eq!(raw.content.runtime.layout().inspect(), before);
                        assert_eq!(raw.content.catalogue.state(), &catalogue);
                        assert_eq!(raw.content.runtime.controller_count(), 2);
                        assert_eq!(raw.content.native.commits, commits);
                        assert_eq!(trace.borrow().factories.len(), factories + 1);
                        continue;
                    }
                    result.unwrap();
                    assert_context_batch_identity(&raw, &trace, factories, providers);
                    assert_eq!(raw.content.native.commits, commits + 1);
                    raw.frame(Vec::new(), false);
                    raw.frame(Vec::new(), false);
                    let reference = raw.host.frame().unwrap().layout.clone();
                    let a = reference.sections.iter().find(|card| card.instance == id("pair-a")).unwrap();
                    let b = reference.sections.iter().find(|card| card.instance == id("pair-b")).unwrap();
                    let c = reference.sections.iter().find(|card| card.instance == id("pair-c")).unwrap();
                    assert_priority_number(a.outer.origin.y, 0.0);
                    assert_priority_number(a.outer.size.height, 180.0);
                    assert_eq!(a.outer.size.width, 280.0);
                    assert_priority_number(b.outer.origin.y, 184.0);
                    assert_priority_number(b.outer.size.height, 616.0);
                    assert_eq!(b.outer.size.width, 280.0);
                    assert_eq!(c.outer, LogicalRect { origin: Point { x: 600.0, y: 100.0 }, size: Size { width: 280.0, height: 240.0 } });
                    assert_eq!(reference.extent, original.extent);
                    assert_eq!(reference.render_domain, original.render_domain);
                    assert_eq!(raw.content.runtime.layout().availability(&id("pair-c")), Availability::Available);
                    raw.frame(Vec::new(), false);
                    raw.frame(Vec::new(), false);
                    assert!(
                        raw.host.frame().unwrap().layout.sections.iter().any(|card| card.instance == id("pair-c")),
                        "actual new floating Area is eventually presented"
                    );
                }
            }
        }
    }
}

#[test]
fn raw_open_or_move_into_stack_before_pair_uses_captured_prefix_geometry_and_true_unavailable_refuses() {
    use commands::LayoutCatalogue;
    for scale in [1.0, 2.0] {
        for direction in [Direction::Ltr, Direction::Rtl] {
            for moving in [false, true] {
                for available in [false, true] {
                    let (mut raw, trace) = context_batch_raw(scale, direction, moving, available);
                    let before = raw.content.runtime.layout().inspect();
                    let catalogue = raw.content.catalogue.state().clone();
                    let factories = trace.borrow().factories.len();
                    let providers = trace.borrow().providers.len();
                    let commits = raw.content.native.commits;
                    let result = run_context_pair_batch(&mut raw, &[context_c_step(!moving, false), context_pair_step(120.0, 408.0)]);
                    if !available {
                        assert_eq!(result.unwrap_err().rule, Rule::Availability);
                        assert_eq!(raw.content.runtime.layout().inspect(), before);
                        assert_eq!(raw.content.catalogue.state(), &catalogue);
                        assert_eq!(raw.content.runtime.controller_count(), 2);
                        assert_eq!(raw.content.native.commits, commits);
                        assert_eq!(trace.borrow().factories.len(), factories + 1);
                        continue;
                    }
                    result.unwrap();
                    assert_context_batch_identity(&raw, &trace, factories, providers);
                    raw.frame(Vec::new(), false);
                    let frame = raw.host.frame().unwrap().layout.clone();
                    for (instance, y, height) in [("pair-a", 0.0, 120.0), ("pair-b", 124.0, 408.0), ("pair-c", 536.0, 264.0)] {
                        let card = frame.sections.iter().find(|card| card.instance == id(instance)).unwrap();
                        assert_priority_number(card.outer.origin.y, y);
                        assert_priority_number(card.outer.size.height, height);
                        assert_eq!(card.outer.size.width, 280.0);
                    }
                    assert_eq!(frame.extent, raw.capacity.size);
                    assert_eq!(raw.content.native.commits, commits + 1);
                    raw.frame(Vec::new(), false);
                    assert_eq!(raw.host.frame().unwrap().layout.sections.len(), 3);
                }
            }
        }
    }
}

#[test]
fn admitted_candidate_rejects_supplied_context_changes_without_replaying_cohort_or_reducer() {
    for change in ["sections", "columns", "priority", "stored_priority", "completeness", "replacement_ownership", "replacement_owner"] {
        let (mut raw, trace) = priority_raw_stack(1.0, Direction::Ltr, Zone::Left, 1000.0);
        let reference = raw.host.command_reference(&raw.content.runtime).unwrap();
        let context = reference.geometry_policy.identity_reservations.preparation_context().unwrap();
        assert!(context.extent_priorities.complete);
        let mut changed = context.clone();
        match change {
            "sections" => {
                changed.sections.insert(section("extra-reserved"));
            }
            "columns" => {
                changed.columns.insert(ColumnId::new("extra-reserved-column").unwrap());
            }
            "priority" => {
                changed.extent_priorities.records[0].priority += 1;
            }
            "stored_priority" => {
                changed.extent_priorities.records[0].stored_priority += 1;
            }
            "completeness" => {
                changed.extent_priorities.complete = false;
            }
            "replacement_ownership" => {
                changed.replacement_ownership = None;
            }
            "replacement_owner" => {
                changed.replacement_ownership.as_mut().unwrap().sections.get_mut(&section("priority-0")).unwrap().insert(id("other-owner"));
            }
            other => panic!("unknown recovery context fixture: {other}"),
        }
        let action = transaction(vec![Mutation::SetSize { section: section("priority-0"), size: Size { width: 280.0, height: 180.0 } }]);
        let before = raw.content.runtime.layout().inspect();
        let commits = raw.content.native.commits;
        let h = &mut raw.content;
        let mut services = ContentServices { models: &h.models, commands: &mut h.commands, local: &mut h.local };
        let mut live = RuntimeActionSink { runtime: &mut h.runtime, native: &mut h.native, services: &mut services };
        let mut sink = super::app::AdmittedActionSink { live: &mut live, host: Some(&mut raw.host), proposal: std::cell::RefCell::new(None) };
        let projected = sink.project_in_context(action.clone(), &context).unwrap();
        assert_eq!(sink.project_in_context(action.clone(), &context).unwrap(), projected);
        let factories = trace.borrow().factories.clone();
        let measures = trace.borrow().measures.clone();
        assert_eq!(sink.dispatch_in_context(action, &changed).unwrap_err().rule, Rule::Revision);
        assert!(sink.proposal.borrow().is_none());
        assert_eq!(sink.inspect(), before);
        assert_eq!(trace.borrow().factories, factories);
        assert_eq!(trace.borrow().measures, measures);
        assert!(!sink.live.native.capabilities().native_windows);
        drop(sink);
        assert_eq!(raw.content.native.commits, commits);
    }
}

fn context_envelope_with_dormant_priority(layout: Layout) -> LayoutEnvelope {
    let mut dormant = layout.instances[0].clone();
    dormant.id = id("recovery-priority");
    dormant.kind = PanelKind::new("extension.priority").unwrap();
    dormant.requested_open = false;
    dormant.memory.section = section("recovery-priority-section");
    dormant.memory.section_index = layout.sections.len();
    dormant.memory.tab_index = 0;
    dormant.memory.preferred_extent = Some(200.0);
    dormant.memory.extent_priority = 17;
    let mut envelope = LayoutEnvelope::new(layout);
    envelope.quarantine.push(QuarantinedPanel {
        instance: dormant.id.clone(),
        kind: dormant.kind.clone(),
        instance_data: serde_json::to_value(dormant).unwrap(),
        section: None,
    });
    envelope
}
struct ContextDriftCatalogue {
    inner: commands::InMemoryCatalogue,
    incoming_source: commands::CatalogueState,
    drift: bool,
    source_changed: bool,
    prepared: std::rc::Rc<std::cell::Cell<usize>>,
    published: std::rc::Rc<std::cell::Cell<usize>>,
    durable: std::rc::Rc<std::cell::RefCell<commands::CatalogueState>>,
}
impl commands::LayoutCatalogue for ContextDriftCatalogue {
    fn state(&self) -> &commands::CatalogueState {
        if self.source_changed { &self.incoming_source } else { commands::LayoutCatalogue::state(&self.inner) }
    }
    fn prepare(&mut self, next: commands::CatalogueState) -> PanelResult<commands::PreparedCatalogue> {
        self.prepared.set(self.prepared.get() + 1);
        let staged = commands::LayoutCatalogue::prepare(&mut self.inner, next)?;
        // Inject a newer opaque recovery-priority context at the REAL storage
        // boundary. IDs, typed live layout, availability and metric sources stay
        // equal. This external source change is distinct from action publication.
        self.source_changed = self.drift;
        Ok(staged)
    }
    fn publish(&mut self, prepared: commands::PreparedCatalogue) {
        self.published.set(self.published.get() + 1);
        commands::LayoutCatalogue::publish(&mut self.inner, prepared);
        *self.durable.borrow_mut() = commands::LayoutCatalogue::state(&self.inner).clone();
    }
}

#[test]
fn actual_catalogue_prepare_context_only_drift_refuses_command_widget_and_owned_pointer_atomically() {
    use commands::LayoutCatalogue;
    for route in ["command", "widget", "pointer"] {
        for drift in [false, true] {
            let (mut h, probe, observations) = boundary_app();
            h.app
                .dispatch_panel_layout(transaction(vec![fixture_open("fixture.log", "context-peer", "context-peer", Zone::Left, 1).unwrap()]))
                .unwrap();
            h.frames(3);
            let registry = h.app.panel_runtime().unwrap().layout().registry().clone();
            let envelope = context_envelope_with_dormant_priority(h.app.panel_runtime().unwrap().layout().layout().clone());
            let mut incoming = envelope.clone();
            let context = preparation_context(&envelope).unwrap();
            let mut recovery = context
                .extent_priorities
                .records
                .iter()
                .find(|record| record.target == ExtentPriorityTarget::Memory(id("recovery-priority")))
                .unwrap()
                .clone();
            recovery.priority += 1;
            incoming.extent_priority_overrides.push(recovery);
            let changed = preparation_context(&incoming).unwrap();
            assert_eq!(changed.sections, context.sections);
            assert_eq!(changed.columns, context.columns);
            assert_eq!(changed.extent_priorities.complete, context.extent_priorities.complete);
            assert_eq!(incoming.layout, envelope.layout);
            assert_eq!(incoming.quarantine, envelope.quarantine);
            assert_ne!(changed.extent_priorities, context.extent_priorities, "only effective recovery context changes");
            let mut catalogue = fixture_command_catalogue(&registry).unwrap();
            catalogue.remember(&envelope, None, &registry).unwrap();
            let mut source = fixture_command_catalogue(&registry).unwrap();
            source.remember(&incoming, None, &registry).unwrap();
            let catalogue_before = catalogue.state().clone();
            let durable = std::rc::Rc::new(std::cell::RefCell::new(catalogue_before.clone()));
            let prepared = std::rc::Rc::new(std::cell::Cell::new(0));
            let published = std::rc::Rc::new(std::cell::Cell::new(0));
            let native = std::rc::Rc::new(std::cell::RefCell::new(FakeNativeAdapter::new(true)));
            h.app
                .panel_runtime_mut()
                .unwrap()
                .set_command_adapters(
                    Box::new(ContextDriftCatalogue {
                        inner: catalogue,
                        incoming_source: source.state().clone(),
                        drift,
                        source_changed: false,
                        prepared: prepared.clone(),
                        published: published.clone(),
                        durable: durable.clone(),
                    }),
                    Box::new(SharedNative(native.clone())),
                )
                .unwrap();
            h.frames(3);
            let before = h.app.panel_runtime().unwrap().layout().inspect();
            let key = app_editor_key(&h.app);
            probe.edit(&key, |state| state.draft = "context refusal retains this live draft".into()).unwrap();
            let retained = probe.state(&key).unwrap();
            let controllers = h.app.panel_runtime().unwrap().controller_count();
            let factories = observations.borrow().factory_keys.clone();
            let doc = h.app.session.doc.clone();
            let model = h.app.session.model.state();
            let revision = h.app.session.revision;
            let undo = h.app.session.undo.len();
            let dirty = h.app.session.is_dirty();
            let camera = h.app.cam;
            if route == "command" {
                let result =
                    h.app.run_panel_command("panel.collapse", &serde_json::json!({"target":{"type":"instance","id":"editor"},"collapsed":true}));
                if drift {
                    assert_eq!(result.unwrap_err().rule, Rule::Revision);
                } else {
                    result.unwrap();
                }
            } else {
                let target = if route == "widget" {
                    HostPointerTarget::Collapse { instance: id("editor") }
                } else {
                    HostPointerTarget::Sash { first: section("editor-section"), second: section("context-peer") }
                };
                let control = h
                    .app
                    .universal_panel_frame()
                    .unwrap()
                    .controls
                    .iter()
                    .find(|control| control.target == HostControlTarget::Pointer(target.clone()))
                    .unwrap();
                let at = egui_rect(control.bounds).center();
                h.frame_with(vec![egui::Event::PointerMoved(at), button_event(at, true)]);
                let end = if route == "pointer" { at + egui::vec2(0.0, 24.0) } else { at };
                if route == "pointer" {
                    h.frame_with(vec![egui::Event::PointerMoved(end)]);
                    let shown = h.app.universal_panel_frame().unwrap().preview.as_ref().expect("exact candidate is shown before storage drift");
                    assert!(shown.rejection.is_none());
                    assert!(shown.projected.is_some());
                    assert_eq!(h.app.panel_runtime().unwrap().layout().inspect(), before);
                }
                h.frame_with(vec![button_event(end, false)]);
                if drift {
                    assert!(
                        h.app
                            .universal_panel_frame()
                            .unwrap()
                            .events
                            .iter()
                            .any(|event| matches!(event, HostEvent::Rejected { error, .. } if error.rule == Rule::Revision))
                    );
                }
            }
            assert_eq!(prepared.get(), 1, "injected catalogue.prepare, not a manual host recapture, changed the context");
            if drift {
                assert_eq!(h.app.panel_runtime().unwrap().layout().inspect(), before);
                assert_eq!(*durable.borrow(), catalogue_before, "the action never publishes its staged catalogue over the external source");
                assert_eq!(published.get(), 0);
                assert_eq!(native.borrow().commits, 0);
                assert!(native.borrow().hosts.is_empty());
                let after = probe.state(&key).unwrap();
                assert_eq!(after.draft, retained.draft);
                assert_eq!(after.transaction_live, retained.transaction_live);
                assert_eq!(
                    (after.cancels, after.reopens, after.reparents, after.destroys),
                    (retained.cancels, retained.reopens, retained.reparents, retained.destroys)
                );
            } else {
                assert_eq!(published.get(), 1);
                assert_eq!(native.borrow().commits, 1);
                assert_eq!(h.app.panel_runtime().unwrap().layout().inspect().revision, before.revision + 1);
            }
            assert_eq!(h.app.panel_runtime().unwrap().controller_count(), controllers);
            assert_eq!(observations.borrow().factory_keys, factories);
            assert!(std::sync::Arc::ptr_eq(&h.app.session.doc, &doc));
            assert!(std::sync::Arc::ptr_eq(&h.app.session.model.state(), &model));
            assert_eq!(h.app.session.revision, revision);
            assert_eq!(h.app.session.undo.len(), undo);
            assert_eq!(h.app.session.is_dirty(), dirty);
            assert_eq!(h.app.cam, camera);
        }
    }
}

#[test]
fn runtime_sink_without_shared_host_forwards_full_recovery_context_and_retains_native_lifecycle() {
    let mut h = PanelMountHarness::new(true).unwrap();
    open_editor(&mut h);
    h.tick().unwrap();
    let key = editor_key(&h);
    h.probe.edit(&key, |state| state.draft = "headless retained transaction".into()).unwrap();
    let envelope = context_envelope_with_dormant_priority(h.runtime.layout().layout().clone());
    h.catalogue.remember(&envelope, None, h.runtime.layout().registry()).unwrap();
    let context = preparation_context(&envelope).unwrap();
    assert!(context.extent_priorities.complete);
    let controllers = h.runtime.controller_count();
    let commits = h.native.commits;
    let before = h.runtime.layout().inspect();
    let action = transaction(vec![Mutation::SetSize { section: section("editor-section"), size: Size { width: 280.0, height: 260.0 } }]);
    let registry = h.runtime.layout().registry().clone();
    {
        let mut services = ContentServices { models: &h.models, commands: &mut h.commands, local: &mut h.local };
        let mut sink = RuntimeActionSink { runtime: &mut h.runtime, native: &mut h.native, services: &mut services };
        let projected = sink.project_in_context(action.clone(), &context).unwrap();
        assert!(projected.sections[0].extent_priority > 17);
        assert_eq!(sink.inspect(), before);
        commands::execute_layout_action(
            action,
            &mut commands::PanelCommandContext { sink: &mut sink, registry: &registry, catalogue: &mut h.catalogue, web: false },
        )
        .unwrap();
    }
    assert!(h.runtime.layout().layout().sections[0].extent_priority > 17);
    assert_eq!(h.runtime.controller_count(), controllers);
    assert_eq!(h.native.commits, commits + 1);
    h.command("panel.popout", &panel_command_native_move()).unwrap();
    assert_eq!(h.native.hosts.len(), 1);
    assert!(h.probe.state(&key).unwrap().reparents > 0);
    h.command("panel.native_close", &serde_json::json!({"target":{"type":"native_host","id":"native-host"}})).unwrap();
    assert!(h.native.hosts.is_empty());
    assert_eq!(h.probe.state(&key).unwrap().draft, "headless retained transaction");
    assert!(h.probe.state(&key).unwrap().transaction_live);
    assert_eq!(h.probe.state(&key).unwrap().cancels, 0);
    assert_eq!(h.commands.calls, 0);
    let current = commands::current_envelope_snapshot(&h.catalogue, &h.runtime.layout().inspect(), &registry).unwrap();
    assert_eq!(current.quarantine, envelope.quarantine, "full context forwarding does not materialize a dormant controller/body");
}

#[test]
fn bare_host_context_is_incomplete_and_only_full_envelope_capture_admits_authored_priority() {
    let mut service = allocation_service(|_| {});
    apply_fixture(&mut service, vec![fixture_open("fixture.log", "context-panel", "context-panel", Zone::Left, 0).unwrap()]);
    let defaults = HostIdentityReservations::default();
    assert!(!defaults.extent_priorities.complete);
    let mut bare = fixture_metric_policy(&service);
    bare.identity_reservations = defaults;
    // Bare allocation remains honest and useful; it is not full-envelope authority
    // for a subsequent authored preference. No provider/factory callback is needed.
    let bounds = LogicalRect { origin: Point { x: 0.0, y: 0.0 }, size: Size { width: 1000.0, height: 800.0 } };
    let host = resolve_host_layout(&service, bounds, Direction::Ltr, HostMetrics::default(), &bare).unwrap();
    let action = transaction(vec![Mutation::SetSize { section: section("context-panel"), size: Size { width: 280.0, height: 200.0 } }]);
    let before = service.inspect();
    assert_eq!(prepare_host_layout(&service, &fixture_scope(&service), &host, action.clone()).unwrap_err().rule, Rule::Capability);
    assert_eq!(service.inspect(), before);
    let captured = fixture_metric_policy(&service);
    assert!(captured.identity_reservations.extent_priorities.complete);
    let host = resolve_host_layout(&service, bounds, Direction::Ltr, HostMetrics::default(), &captured).unwrap();
    let proposal = prepare_host_layout(&service, &fixture_scope(&service), &host, action).unwrap();
    assert_eq!(proposal.prepared().layout().sections[0].preferred_extent, Some(200.0));
    assert!(proposal.prepared().layout().sections[0].extent_priority > 0);
    assert_eq!(service.inspect(), before);
}

#[test]
fn app_atomic_workspace_replacements_accept_distinct_incoming_contexts_and_match_headless_commit() {
    use commands::LayoutCatalogue;
    let mut outcomes = Vec::new();
    for admitted in [false, true] {
        let (mut h, trace) = review_cohort_host_with_layout(
            |_| {},
            |service| {
                *service = LayoutService::new(
                    service.registry().clone(),
                    service.layout().clone(),
                    HostCapabilities { native_windows: true, max_native_hosts: 8, persistence: true },
                )
                .unwrap();
                apply_fixture(
                    service,
                    vec![
                        fixture_open("fixture.log", "log", "log-section", Zone::Right, 0).unwrap(),
                        fixture_open("fixture.editor", "editor", "editor-section", Zone::Left, 1).unwrap(),
                    ],
                );
            },
        );
        h.native = FakeNativeAdapter::new(true);
        h.tick().unwrap();
        let key = editor_key(&h);
        h.probe
            .edit(&key, |state| {
                state.draft = "replacement batch retains this transaction".into();
                state.transaction_live = true;
            })
            .unwrap();
        let retained = h.probe.state(&key).unwrap();
        let before = h.runtime.layout().inspect();
        let registry = h.runtime.layout().registry().clone();
        let mut catalogue = fixture_command_catalogue(&registry).unwrap();
        catalogue
            .register_workspace(commands::WorkspaceDescriptor {
                id: WorkspaceId::new("review.final").unwrap(),
                title_key: "ui.panel".into(),
                default_preset: "Fixture default".into(),
            })
            .unwrap();
        let mut contexts = Vec::new();
        for (workspace, reserved_section, reserved_column, rank, native) in [
            ("fixture.other", "incoming-a-reserved", "incoming-a-column", 31, false),
            ("review.final", "incoming-b-reserved", "incoming-b-column", 47, true),
        ] {
            let mut layout = before.layout.clone();
            layout.workspace = WorkspaceId::new(workspace).unwrap();
            if native {
                let mut service = LayoutService::new(registry.clone(), layout, before.capabilities.clone()).unwrap();
                apply_fixture(
                    &mut service,
                    vec![Mutation::Move {
                        instance: id("editor"),
                        destination: serde_json::from_value(panel_command_native_move()["destination"].clone()).unwrap(),
                    }],
                );
                layout = service.layout().clone();
            }
            let mut envelope = context_envelope_with_dormant_priority(layout);
            let memory = &mut envelope.quarantine[0].instance_data["memory"];
            memory["section"] = serde_json::json!(reserved_section);
            memory["column"] = serde_json::json!(reserved_column);
            memory["column_width"] = serde_json::json!(280.0);
            let mut recovery = preparation_context(&envelope)
                .unwrap()
                .extent_priorities
                .records
                .into_iter()
                .find(|record| record.target == ExtentPriorityTarget::Memory(id("recovery-priority")))
                .unwrap();
            recovery.priority = rank;
            envelope.extent_priority_overrides.push(recovery);
            let context = preparation_context(&envelope).unwrap();
            assert!(context.sections.contains(&section(reserved_section)));
            assert!(context.columns.contains(&ColumnId::new(reserved_column).unwrap()));
            contexts.push(context);
            catalogue.remember(&envelope, None, &registry).unwrap();
        }
        assert_ne!(contexts[0].sections, contexts[1].sections);
        assert_ne!(contexts[0].columns, contexts[1].columns);
        assert_ne!(contexts[0].extent_priorities, contexts[1].extent_priorities);
        let durable = std::rc::Rc::new(std::cell::RefCell::new(catalogue.state().clone()));
        let prepared = std::rc::Rc::new(std::cell::Cell::new(0));
        let published = std::rc::Rc::new(std::cell::Cell::new(0));
        let mut counted = ContextDriftCatalogue {
            incoming_source: catalogue.state().clone(),
            inner: catalogue,
            drift: false,
            source_changed: false,
            prepared: prepared.clone(),
            published: published.clone(),
            durable: durable.clone(),
        };
        let steps = [
            serde_json::json!({"command":"layout.workspace.select","params":{"workspace":"fixture.other"}}),
            serde_json::json!({"command":"layout.workspace.select","params":{"workspace":"review.final"}}),
            serde_json::json!({"command":"layout.get"}),
        ];
        let factories = trace.borrow().factories.len();
        let (reply, state, count, hosts, commits) = if admitted {
            let native = std::rc::Rc::new(std::cell::RefCell::new(FakeNativeAdapter::new(true)));
            let mut session = solidcraft_engine::Session::default();
            session.document_id = "fixture.document".into();
            let mut app = crate::SolveApp::new(session, Default::default());
            let doc = app.session.doc.clone();
            let model = app.session.model.state();
            let camera = app.cam;
            let revision = app.session.revision;
            let undo = app.session.undo.len();
            let dirty = app.session.is_dirty();
            app.enable_panel_runtime(h.runtime, Box::new(FakeAppPanelServices { models: FakeModels { editable: true }, ..Default::default() }))
                .unwrap();
            app.panel_runtime_mut().unwrap().set_command_adapters(Box::new(counted), Box::new(SharedCommandNative(native.clone()))).unwrap();
            prepare_app_commands(&mut app);
            let reply = app.run_panel_batch(&steps).unwrap();
            assert!(std::sync::Arc::ptr_eq(&app.session.doc, &doc));
            assert!(std::sync::Arc::ptr_eq(&app.session.model.state(), &model));
            assert_eq!(app.cam, camera);
            assert_eq!(app.session.revision, revision);
            assert_eq!(app.session.undo.len(), undo);
            assert_eq!(app.session.is_dirty(), dirty);
            let runtime = app.panel_runtime().unwrap();
            let adapter = native.borrow();
            (reply, runtime.layout().inspect(), runtime.controller_count(), adapter.hosts.clone(), adapter.commits)
        } else {
            let mut services = ContentServices { models: &h.models, commands: &mut h.commands, local: &mut h.local };
            let mut sink = RuntimeActionSink { runtime: &mut h.runtime, native: &mut h.native, services: &mut services };
            let reply = commands::execute_batch(
                &steps,
                &mut commands::PanelCommandContext { sink: &mut sink, registry: &registry, catalogue: &mut counted, web: false },
            )
            .unwrap();
            (reply, h.runtime.layout().inspect(), h.runtime.controller_count(), h.native.hosts.clone(), h.native.commits)
        };
        assert_eq!(reply["completed"], 3);
        assert_eq!(state.revision, before.revision + 1);
        assert_eq!(state.layout.workspace.0, "review.final");
        assert_eq!(prepared.get(), 1);
        assert_eq!(published.get(), 1);
        assert_eq!(commits, 1);
        let saved = durable.borrow().remembered(&state.layout.workspace).unwrap().project_envelope(&registry, &state.layout.workspace).unwrap();
        let saved_context = preparation_context(&saved).unwrap();
        assert!(saved_context.sections.contains(&section("incoming-b-reserved")));
        assert!(saved_context.columns.contains(&ColumnId::new("incoming-b-column").unwrap()));
        assert_eq!(
            saved_context
                .extent_priorities
                .records
                .iter()
                .find(|record| record.target == ExtentPriorityTarget::Memory(id("recovery-priority")))
                .unwrap()
                .priority,
            47
        );
        assert_eq!(hosts.get(&HostId::new("native-host").unwrap()), Some(&section("native")));
        assert_eq!(count, 3, "retain outgoing owners and publish only the final incoming workspace controller");
        assert_eq!(trace.borrow().factories.len(), factories + if admitted { 2 } else { 1 });
        for workspace in ["fixture.other", "review.final"] {
            let owner = ControllerKey { instance: id("log"), owner: ContentOwner::Workspace(WorkspaceId::new(workspace).unwrap()) };
            let serials: Vec<_> = trace.borrow().factories.iter().filter(|(key, _)| *key == owner).map(|(_, serial)| *serial).collect();
            if workspace == "fixture.other" && !admitted {
                assert!(serials.is_empty(), "headless planning creates only the final cohort");
                continue;
            }
            assert_eq!(serials.len(), 1, "each proposed owner is created once");
            let serial = serials[0];
            assert_eq!(
                trace.borrow().measures.iter().filter(|measured| **measured == serial).count(),
                1,
                "repeated projection/final dispatch reuse the staged provider"
            );
            if workspace == "fixture.other" {
                assert!(h.probe.state(&owner).is_err(), "intermediate owner never publishes");
                assert!(!trace.borrow().lifecycle.iter().any(|(identity, _)| *identity == serial));
            } else {
                assert!(h.probe.state(&owner).unwrap().last_state.is_some());
            }
        }
        let after = h.probe.state(&key).unwrap();
        assert_eq!(after.draft, retained.draft);
        assert!(after.transaction_live);
        assert_eq!(after.reparents, retained.reparents + 1);
        assert_eq!((after.cancels, after.reopens, after.destroys), (retained.cancels, retained.reopens, retained.destroys));
        outcomes.push((reply, state, durable.borrow().clone(), hosts));
    }
    assert_eq!(outcomes[0], outcomes[1], "app admission and headless planning publish the same final envelope and native placement");
}

#[test]
fn captured_renderer_alpha_reaches_new_sections_and_alpha_only_drift_invalidates_owned_geometry() {
    for mode in [crate::ui_kit::LayoutMode::Floating, crate::ui_kit::LayoutMode::Docked] {
        let (mut h, _, _) = boundary_app();
        h.app.ui.layout = mode;
        h.frames(2);
        let painted = h.app.universal_panel_frame().unwrap().layout.clone();
        let captured_style = painted.geometry_policy.surfaces.as_ref().unwrap();
        let alpha = HostSurfaceAlpha {
            docked: captured_style.appearance(PanelSurface::FollowZone, false).fill.a(),
            floating: captured_style.appearance(PanelSurface::FollowZone, true).fill.a(),
        };
        assert_eq!(painted.geometry_policy.surface_alpha, Some(alpha), "the actual app captures the shared renderer policy before allocation");
        for card in &painted.sections {
            let appearance = captured_style.appearance(card.surface, card.peek || card.zone == Zone::Floating);
            assert_eq!(card.paint_alpha, Some(appearance.fill.a()));
            assert_eq!(card.contrast, appearance.contrast);
            assert_eq!(card.paint_alpha, Some(255), "Classic's default opaque policy is preserved");
        }
        let gap = crate::ui_kit::Container::for_layout(mode, crate::ui_kit::Surface::Panel).gap();
        assert_eq!(gap, if mode == crate::ui_kit::LayoutMode::Floating { 7.0 } else { 0.0 });
        assert_eq!(painted.metrics.gap, gap);
        assert_eq!(painted.allocation_environment.gap, gap);
        let domain = egui_rect(painted.render_domain);
        let capacity = egui_rect(painted.capacity);
        assert_eq!(h.app.shell.content, Some(domain));
        assert_eq!(painted.viewport, Some(painted.render_domain));
        assert_eq!(capacity.left(), domain.left() + gap);
        assert_eq!(capacity.right(), domain.right() - gap);
        assert_eq!(capacity.top(), h.app.shell.cards[1].bottom() + gap);
        assert_eq!(capacity.bottom(), h.app.shell.status_bar.map_or(domain.bottom(), |bar| bar.top()) - gap);

        let before = h.app.panel_runtime().unwrap().layout().inspect();
        let doc = h.app.session.doc.clone();
        let model = h.app.session.model.state();
        let camera = h.app.cam;
        let revision = h.app.session.revision;
        let undo = h.app.session.undo.len();
        let dirty = h.app.session.is_dirty();
        let runtime = h.app.panel_runtime().unwrap();
        let service = runtime.layout();
        let environment = runtime.environment();
        let scope =
            LocalScope { workspace: environment.workspace.clone(), document: environment.document.clone(), viewport: environment.viewport.clone() };
        // Reuse the analytic metrics fixture for not-yet-created content. The
        // alpha and shell geometry above come from the real app preparation.
        let mut policy = fixture_metric_policy(service);
        policy.surface_alpha = painted.geometry_policy.surface_alpha;
        let resolve = |policy: &HostGeometryPolicy| {
            resolve_host_layout_in_domain(
                service,
                painted.render_domain,
                painted.capacity,
                &painted.chrome_obstacles,
                painted.direction,
                painted.metrics,
                policy,
            )
            .unwrap()
        };
        let reference = resolve(&policy);
        let mut styled_policy = policy.clone();
        styled_policy.default_opaque = false;
        styled_policy.surfaces = Some(HostSurfaceStyle::capture(&h.ctx, false, Some(0.5), None));
        let styled = resolve(&styled_policy);
        for card in &styled.sections {
            let appearance = styled_policy.surfaces.as_ref().unwrap().appearance(card.surface, card.peek || card.zone == Zone::Floating);
            assert_eq!(card.paint_alpha, Some(appearance.fill.a()));
            assert_eq!(card.contrast, appearance.contrast);
        }
        assert_eq!(styled.render_domain, reference.render_domain);
        assert_eq!(styled.controls, reference.controls, "appearance leaves all pointer bounds and identities intact");
        let action = transaction(vec![
            fixture_open("fixture.log", "alpha-dock", "alpha-dock", Zone::Right, 1).unwrap(),
            Mutation::Open {
                kind: PanelKind::new("fixture.log").unwrap(),
                instance: id("alpha-float"),
                destination: Some(Destination::NewSection {
                    section: section("alpha-float"),
                    index: 2,
                    location: SectionLocation::Floating {
                        origin: Point { x: painted.capacity.origin.x + 400.0, y: painted.capacity.origin.y + 80.0 },
                    },
                    size: Size { width: 280.0, height: 240.0 },
                }),
            },
        ]);
        let candidate = prepare_host_layout(service, &scope, &reference, action.clone()).unwrap();
        candidate.validate_current(service, &scope, &reference).unwrap();
        for (instance, expected) in [("alpha-dock", alpha.docked), ("alpha-float", alpha.floating)] {
            assert!(reference.sections.iter().all(|card| card.instance != id(instance)));
            assert_eq!(candidate.layout().sections.iter().find(|card| card.instance == id(instance)).unwrap().paint_alpha, Some(expected));
        }
        let changed_alpha = HostSurfaceAlpha { docked: alpha.docked.wrapping_add(1), floating: alpha.floating.wrapping_add(2) };
        policy.surface_alpha = Some(changed_alpha);
        let changed = resolve(&policy);
        assert_eq!(candidate.validate_current(service, &scope, &changed).unwrap_err().rule, Rule::Revision);
        assert_eq!(changed.controls, reference.controls, "alpha drift preserves widget identities and bounds");
        let mut restored = changed.clone();
        restored.geometry_policy.surface_alpha = Some(alpha);
        for card in &mut restored.sections {
            card.paint_alpha = Some(alpha.docked);
        }
        assert_eq!(restored, reference, "only captured fill alpha changed, including its section annotation");
        let revised = prepare_host_layout(service, &scope, &changed, action.clone()).unwrap();
        for (instance, expected) in [("alpha-dock", changed_alpha.docked), ("alpha-float", changed_alpha.floating)] {
            let card = revised.layout().sections.iter().find(|card| card.instance == id(instance)).unwrap();
            let original = candidate.layout().sections.iter().find(|card| card.instance == id(instance)).unwrap();
            assert_eq!(card.paint_alpha, Some(expected));
            assert_eq!(card.surface, original.surface, "paint alpha does not rewrite surface intent");
        }
        assert_eq!(HostGeometryPolicy::default().surface_alpha, None);
        policy.surface_alpha = None;
        let unknown = prepare_host_layout(service, &scope, &resolve(&policy), action).unwrap();
        assert!(unknown.layout().sections.iter().all(|card| card.paint_alpha.is_none()), "a renderer-free policy cannot claim a known alpha");
        assert_eq!(service.inspect(), before);
        assert!(std::sync::Arc::ptr_eq(&h.app.session.doc, &doc));
        assert!(std::sync::Arc::ptr_eq(&h.app.session.model.state(), &model));
        assert_eq!(h.app.cam, camera);
        assert_eq!(h.app.session.revision, revision);
        assert_eq!(h.app.session.undo.len(), undo);
        assert_eq!(h.app.session.is_dirty(), dirty);
    }
}

#[test]
fn shared_host_accessibility_exposes_tabs_disclosure_and_oriented_splits() {
    for scale in [1.0, 2.0] {
        let mut content = host_logs(2, Zone::Left);
        content
            .dispatch(vec![Mutation::Open {
                kind: PanelKind::new("fixture.log").unwrap(),
                instance: id("extra-tab"),
                destination: Some(Destination::JoinTab { section: section("section-0"), index: 1 }),
            }])
            .unwrap();
        let mut raw = RawPanelHost::new(content, scale);
        raw.frame(Vec::new(), false);
        let (frame, output) = raw.frame(Vec::new(), false);
        let nodes = &output.platform_output.accesskit_update.as_ref().unwrap().nodes;
        let active = frame.layout.sections.iter().find(|card| card.section == section("section-0")).unwrap().instance.clone();
        let tabs: Vec<_> =
            frame.controls.iter().filter(|control| matches!(control.target, HostControlTarget::Pointer(HostPointerTarget::Tab { .. }))).collect();
        assert_eq!(tabs.len(), 2);
        for tab in tabs {
            let node = &nodes.iter().find(|(id, _)| *id == tab.id.accesskit_id()).unwrap().1;
            assert_eq!(node.role(), egui::accesskit::Role::Tab);
            assert_eq!(
                node.is_selected(),
                Some(matches!(&tab.target, HostControlTarget::Pointer(HostPointerTarget::Tab { instance }) if *instance == active))
            );
        }
        let tablist = nodes.iter().find(|(_, node)| node.role() == egui::accesskit::Role::TabList).unwrap();
        let mut descendants = vec![tablist.0];
        let mut next = 0;
        while next < descendants.len() && next < nodes.len() {
            if let Some((_, node)) = nodes.iter().find(|(id, _)| *id == descendants[next]) {
                for child in node.children() {
                    if !descendants.contains(child) {
                        descendants.push(*child);
                    }
                }
            }
            next += 1;
        }
        for tab in frame.controls.iter().filter(|control| matches!(control.target, HostControlTarget::Pointer(HostPointerTarget::Tab { .. }))) {
            assert!(descendants.contains(&tab.id.accesskit_id()), "the actual Tab must descend from the actual TabList");
        }
        let collapse = raw_control(&raw, &HostControlTarget::Pointer(HostPointerTarget::Collapse { instance: active }));
        assert_eq!(nodes.iter().find(|(id, _)| *id == collapse.id.accesskit_id()).unwrap().1.is_expanded(), Some(true));
        assert!(
            nodes
                .iter()
                .any(|(_, node)| node.role() == egui::accesskit::Role::Splitter && node.orientation().is_some() && node.numeric_value().is_some())
        );
    }
}

fn sash_numeric_value(frame: &HostFrame, output: &egui::FullOutput, target: &HostControlTarget) -> f64 {
    let control = frame.controls.iter().find(|control| control.target == *target).unwrap();
    output
        .platform_output
        .accesskit_update
        .as_ref()
        .unwrap()
        .nodes
        .iter()
        .find(|(id, _)| *id == control.id.accesskit_id())
        .unwrap()
        .1
        .numeric_value()
        .unwrap()
}

#[test]
fn contextual_keyboard_resize_reuses_candidates_and_rejects_foreign_focus() {
    for scale in [1.0, 2.0] {
        for direction in [Direction::Ltr, Direction::Rtl] {
            let mut content = host_logs(2, Zone::Left);
            let mut environment = content.runtime.environment().clone();
            environment.direction = direction;
            content.runtime.set_environment(environment, &mut content.local).unwrap();
            let mut raw = RawPanelHost::new(content, scale);
            raw.frame(Vec::new(), false);
            let (frame, output) = raw.frame(Vec::new(), false);
            let sash =
                frame.controls.iter().find(|control| matches!(control.target, HostControlTarget::Pointer(HostPointerTarget::Sash { .. }))).unwrap();
            let target = sash.target.clone();
            let before_value = sash_numeric_value(&frame, &output, &target);
            raw.ctx.memory_mut(|memory| memory.request_focus(sash.id));
            raw.frame(Vec::new(), false);
            let candidate = raw.host.contextual_action(&raw.content.runtime, ContextualPanelAction::ResizeDown).unwrap();
            let before = raw.content.runtime.layout().inspect();
            raw.frame(vec![host_key(egui::Key::ArrowDown, true)], false);
            let (after_frame, output) = raw.frame(vec![host_key(egui::Key::ArrowDown, false)], false);
            assert_ne!(raw.content.runtime.layout().inspect().revision, before.revision);
            assert!(matches!(candidate, LayoutAction::Apply { .. }));
            let value = sash_numeric_value(&after_frame, &output, &target);
            assert_eq!(value, f64::from(after_frame.layout.sections[0].outer.size.height));
            assert!(value > before_value, "the default standalone arrow changes the controlled height and accessible value");
            raw.host.set_keyboard_shortcuts(Vec::new());
            let unbound = raw.content.runtime.layout().inspect();
            raw.frame(vec![host_key(egui::Key::ArrowDown, true)], false);
            raw.frame(vec![host_key(egui::Key::ArrowDown, false)], false);
            assert_eq!(raw.content.runtime.layout().inspect(), unbound, "application bindings replace rather than extend standalone defaults");
            raw.foreign_cover = Some(egui_rect(raw.capacity));
            raw.foreign_takes_focus = true;
            raw.frame(Vec::new(), false);
            raw.frame(Vec::new(), false);
            assert!(raw.host.contextual_action(&raw.content.runtime, ContextualPanelAction::ResizeDown).is_err());
        }
    }
}

#[test]
fn splitter_accessible_extent_keeps_its_axis_and_value_when_the_host_scrolls() {
    for scale in [1.0, 2.0] {
        for direction in [Direction::Ltr, Direction::Rtl] {
            for zone in [Zone::Left, Zone::Bottom] {
                let mut content = host_logs(2, zone);
                let mut environment = content.runtime.environment().clone();
                environment.direction = direction;
                content.runtime.set_environment(environment, &mut content.local).unwrap();
                let mut raw = RawPanelHost::new(content, scale);
                raw.capacity.size = if zone == Zone::Bottom { Size { width: 250.0, height: 400.0 } } else { Size { width: 800.0, height: 200.0 } };
                raw.frame(Vec::new(), false);
                let (before_frame, output) = raw.frame(Vec::new(), false);
                let target = HostControlTarget::Pointer(HostPointerTarget::Sash { first: section("section-0"), second: section("section-1") });
                let value = sash_numeric_value(&before_frame, &output, &target);
                let before_control = raw_control(&raw, &target);
                let horizontal_scroll = zone == Zone::Bottom;
                let track = raw_control(&raw, &HostControlTarget::Scroll { horizontal: horizontal_scroll });
                let bounds = egui_rect(track.bounds);
                let at = if horizontal_scroll { bounds.right_center() - egui::vec2(1.0, 0.0) } else { bounds.center_bottom() - egui::vec2(0.0, 1.0) };
                let before = raw.content.runtime.layout().inspect();
                raw.frame(vec![egui::Event::PointerMoved(at)], false);
                raw.pointer(at, true, false);
                raw.pointer(at, false, false);
                let (after_frame, output) = raw.frame(Vec::new(), false);
                let after_control = raw_control(&raw, &target);
                assert_ne!(after_control.bounds.origin, before_control.bounds.origin, "the actual sash moved with presentation scrolling");
                assert_eq!(sash_numeric_value(&after_frame, &output, &target), value, "scrolling cannot announce a panel resize");
                let node = &output
                    .platform_output
                    .accesskit_update
                    .as_ref()
                    .unwrap()
                    .nodes
                    .iter()
                    .find(|(id, _)| *id == after_control.id.accesskit_id())
                    .unwrap()
                    .1;
                assert_eq!(
                    node.orientation(),
                    Some(if horizontal_scroll { egui::accesskit::Orientation::Vertical } else { egui::accesskit::Orientation::Horizontal })
                );
                assert_eq!(raw.content.runtime.layout().inspect(), before);
            }
        }
    }
}

#[test]
fn contextual_panel_focus_reveals_last_tiny_header_without_a_canvas_shield() {
    for scale in [1.0, 2.0] {
        let mut content = host_logs(24, Zone::Left);
        content
            .dispatch((0..24).map(|index| Mutation::Collapse { section: section(&format!("section-{index}")), collapsed: true }).collect())
            .unwrap();
        let mut raw = RawPanelHost::new(content, scale);
        raw.capacity.size = Size { width: 480.0, height: 320.0 };
        raw.frame(Vec::new(), false);
        raw.frame(Vec::new(), false);
        let first = raw_control(&raw, &HostControlTarget::Pointer(HostPointerTarget::Header { instance: id("log-0") }));
        raw.ctx.memory_mut(|memory| memory.request_focus(first.id));
        raw.frame(Vec::new(), false);
        let action = raw.host.contextual_action(&raw.content.runtime, ContextualPanelAction::FocusPrevious).unwrap();
        let reply = raw
            .content
            .dispatch(match action {
                LayoutAction::Apply { mutations, .. } => mutations,
                _ => panic!("focus must be a typed mutation"),
            })
            .unwrap();
        raw.host.note_reply(&reply);
        raw.frame(Vec::new(), false);
        let frame = raw.frame(Vec::new(), false).0;
        let last = raw_control(&raw, &HostControlTarget::Pointer(HostPointerTarget::Header { instance: id("log-23") }));
        assert_eq!(raw.ctx.memory(|memory| memory.focused()), Some(last.id));
        assert!(raw.capacity.contains_rect(last.bounds));
        assert!(frame.layout.input.iter().all(|bounds| frame.layout.painted.contains(bounds)));
        assert!(frame.layout.sections.iter().filter(|card| card.instance != id("log-23")).all(|card| card.collapsed && card.body.is_none()));
        let target = frame.layout.sections.iter().find(|card| card.instance == id("log-23")).unwrap();
        assert!(!target.collapsed && target.body.is_some(), "Focus deliberately expands only its target");
    }
}

#[test]
fn shared_menu_keyboard_entry_end_and_escape_restore_actual_opener() {
    for scale in [1.0, 2.0] {
        let mut raw = RawPanelHost::new(host_logs(1, Zone::Left), scale);
        raw.ctx.global_style_mut(|style| style.scroll_animation = egui::style::ScrollAnimation::none());
        // The compact menu still needs enough overflow to scroll a whole row out of view.
        raw.capacity.size = Size { width: 480.0, height: 64.0 };
        raw.frame(Vec::new(), false);
        raw.frame(Vec::new(), false);
        let opener = raw_control(&raw, &HostControlTarget::Pointer(HostPointerTarget::Menu { instance: id("log-0") }));
        raw.ctx.memory_mut(|memory| memory.request_focus(opener.id));
        raw.frame(vec![host_key(egui::Key::Enter, true)], false);
        raw.frame(vec![host_key(egui::Key::Enter, false)], false);
        raw.frame(Vec::new(), false);
        assert_ne!(raw.ctx.memory(|memory| memory.focused()), Some(opener.id));
        // Settle the entry reveal, then prove a retained first-row focus does
        // not undo manual wheel scrolling on subsequent ordinary paints.
        raw.frame(Vec::new(), false);
        let first = raw.focused_response.as_ref().unwrap().clone();
        scroll_real_host_popup_to_end(&mut raw);
        assert_eq!(raw.ctx.memory(|memory| memory.focused()), Some(first.id));
        let scrolled = raw.focused_response.as_ref().unwrap().clone();
        let inner = egui_rect(raw.host.frame().unwrap().menu_bounds.unwrap()).shrink(solidcraft_theme::Tokens::get().space_6);
        assert!(scrolled.rect.bottom() < inner.top(), "manual scrolling can leave the focused first item outside the visible menu");
        raw.frame(Vec::new(), false);
        assert_eq!(raw.focused_response.as_ref().unwrap().rect, scrolled.rect, "idle repaint must not pull manual scroll back to the focused row");
        let (_, output) = raw.frame(vec![host_key(egui::Key::End, true)], false);
        assert!(output.platform_output.accesskit_update.as_ref().unwrap().nodes.iter().any(|(_, node)| node.role() == egui::accesskit::Role::Menu));
        raw.frame(vec![host_key(egui::Key::End, false)], false);
        raw.frame(Vec::new(), false);
        raw.frame(Vec::new(), false);
        let focused = raw.ctx.memory(|memory| memory.focused()).unwrap();
        // read_response after run_ui can return the preceding pass: egui swaps
        // its current/previous widget stores at end_pass. Use the actual
        // response already captured inside this frame after menu painting.
        let response = raw.focused_response.as_ref().unwrap();
        assert_eq!(response.id, focused);
        let menu = egui_rect(raw.host.frame().unwrap().menu_bounds.unwrap());
        let inner = menu.shrink(solidcraft_theme::Tokens::get().space_6);
        assert!(inner.contains_rect(response.rect), "final action {:?} should fit in the menu {inner:?}", response.rect);
        assert!(response.interact_rect.is_positive());
        assert_eq!(response.interact_rect, response.rect, "the focused row must be fully interactable too");
        raw.frame(vec![host_key(egui::Key::Escape, true)], false);
        raw.frame(vec![host_key(egui::Key::Escape, false)], false);
        assert_eq!(raw.ctx.memory(|memory| memory.focused()), Some(opener.id));
        assert!(raw.host.frame().unwrap().menu_bounds.is_none());
    }
}

#[test]
fn nested_menu_keyboard_entry_and_escape_keep_the_live_root_opener() {
    for scale in [1.0, 2.0] {
        for fixture in [PopupFixture::Tabs, PopupFixture::CompactTabs] {
            let (mut raw, source, button) = raw_popup_fixture(scale, fixture);
            let root = raw_control(&raw, &source);
            open_root_popup(&mut raw, &source, button, false);
            raw.frame(Vec::new(), false);
            let nested = raw_control(&raw, &HostControlTarget::PanelMenu { instance: id("log-0") });
            raw.ctx.memory_mut(|memory| memory.request_focus(nested.id));
            raw.frame(Vec::new(), false);
            raw.frame(vec![host_key(egui::Key::Enter, true)], false);
            raw.frame(vec![host_key(egui::Key::Enter, false)], false);
            raw.frame(Vec::new(), false);
            let first = raw_control(&raw, &HostControlTarget::Shared { instance: id("log-0"), action: HostSharedAction::Focus });
            assert_eq!(
                raw.ctx.memory(|memory| memory.focused()),
                Some(first.id),
                "the replacement panel menu takes keyboard focus from its immediate submenu source"
            );
            assert!(
                !raw.host.frame().unwrap().controls.iter().any(|control| control.target == HostControlTarget::PanelMenu { instance: id("log-0") })
            );
            raw.frame(vec![host_key(egui::Key::ArrowDown, true)], false);
            raw.frame(vec![host_key(egui::Key::ArrowDown, false)], false);
            let second = raw_control(&raw, &HostControlTarget::Shared { instance: id("log-0"), action: HostSharedAction::Collapse });
            assert_eq!(raw.ctx.memory(|memory| memory.focused()), Some(second.id), "the entered submenu is immediately keyboard navigable");
            raw.frame(vec![host_key(egui::Key::Escape, true)], false);
            raw.frame(vec![host_key(egui::Key::Escape, false)], false);
            assert_no_popup(raw.host.frame().unwrap());
            assert_eq!(
                raw.ctx.memory(|memory| memory.focused()),
                Some(root.id),
                "Escape restores the live overflow launcher, not the vanished submenu row"
            );
            // A separate fresh menu must establish a new root launcher.
            let fresh = raw_control(&raw, &HostControlTarget::Pointer(HostPointerTarget::Menu { instance: id("log-0") }));
            raw.ctx.memory_mut(|memory| memory.request_focus(fresh.id));
            raw.frame(Vec::new(), false);
            raw.frame(vec![host_key(egui::Key::Enter, true)], false);
            raw.frame(vec![host_key(egui::Key::Enter, false)], false);
            raw.frame(Vec::new(), false);
            raw.frame(vec![host_key(egui::Key::Escape, true)], false);
            raw.frame(vec![host_key(egui::Key::Escape, false)], false);
            assert_eq!(raw.ctx.memory(|memory| memory.focused()), Some(fresh.id));
        }
    }
}

#[test]
fn focused_editor_keeps_automatic_keys_and_explicit_panel_intents_are_observational() {
    for scale in [1.0, 2.0] {
        let (mut content, trace) = review_cohort_host(|_| {});
        content.dispatch(vec![fixture_open("fixture.editor", "wrapping-editor", "wrapping", Zone::Left, 0).unwrap()]).unwrap();
        let mut raw = RawPanelHost::new(content, scale);
        raw.frame(Vec::new(), false);
        raw.frame(Vec::new(), false);
        let editor = trace.borrow().editors.last().unwrap().clone();
        raw.ctx.memory_mut(|memory| memory.request_focus(editor.0));
        raw.frame(Vec::new(), false);
        raw.host.set_keyboard_shortcuts(vec![
            (ContextualPanelAction::Toggle, egui::KeyboardShortcut::new(egui::Modifiers::NONE, egui::Key::T)),
            (ContextualPanelAction::FocusNext, egui::KeyboardShortcut::new(egui::Modifiers::NONE, egui::Key::ArrowLeft)),
        ]);
        let before = raw.content.runtime.layout().inspect();
        let (invalidated, _) = raw.frame_late(vec![host_key(egui::Key::T, true), egui::Event::Text("typed text".into())], false, |_, _| {});
        assert!(invalidated.is_none(), "changed editor text invalidates its captured metrics");
        raw.frame(Vec::new(), false);
        raw.frame(vec![host_key(egui::Key::T, false), host_key(egui::Key::ArrowLeft, true)], false);
        raw.frame(vec![host_key(egui::Key::ArrowLeft, false)], false);
        assert_eq!(raw.ctx.memory(|memory| memory.focused()), Some(editor.0));
        assert!(trace.borrow().editors.last().unwrap().3.contains("typed text"));
        assert_eq!(raw.content.runtime.layout().inspect(), before);
        let caret = egui::TextEdit::load_state(&raw.ctx, editor.0).unwrap().cursor.char_range();
        let explicit = raw.host.contextual_action(&raw.content.runtime, ContextualPanelAction::Toggle).unwrap();
        assert!(matches!(explicit, LayoutAction::Apply { .. }));
        assert_eq!(raw.content.runtime.layout().inspect(), before, "resolving a command must not dispatch it");
        assert!(raw.host.contextual_action(&raw.content.runtime, ContextualPanelAction::ResizeLeft).is_err());
        assert_eq!(egui::TextEdit::load_state(&raw.ctx, editor.0).unwrap().cursor.char_range(), caret);
        assert_eq!(raw.content.commands.calls, 0);
    }
}

#[test]
fn content_focus_bookmark_restores_caret_after_explicit_collapse_and_focus() {
    for scale in [1.0, 2.0] {
        let (mut content, trace) = review_cohort_host(|_| {});
        content.dispatch(vec![fixture_open("fixture.editor", "wrapping-editor", "wrapping", Zone::Left, 0).unwrap()]).unwrap();
        let mut raw = RawPanelHost::new(content, scale);
        raw.frame(Vec::new(), false);
        raw.frame(Vec::new(), false);
        let editor = trace.borrow().editors.last().unwrap().clone();
        raw.ctx.memory_mut(|memory| memory.request_focus(editor.0));
        raw.frame(Vec::new(), false);
        let (invalidated, _) = raw.frame_late(vec![egui::Event::Text(" retained selection".into())], false, |_, _| {});
        assert!(invalidated.is_none(), "changed editor text invalidates its captured metrics");
        raw.frame(Vec::new(), false);
        let before = trace.borrow().editors.last().unwrap().3.clone();
        let expected: String = before.chars().take(1).chain(std::iter::once('!')).chain(before.chars().skip(4)).collect();
        let mut state = egui::TextEdit::load_state(&raw.ctx, editor.0).unwrap();
        state.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(1), egui::text::CCursor::new(4))));
        state.store(&raw.ctx, editor.0);
        raw.frame(Vec::new(), false);
        let caret = egui::TextEdit::load_state(&raw.ctx, editor.0).unwrap().cursor.char_range();
        assert!(!caret.unwrap().is_empty(), "exercise an actual nonempty selection before hiding");
        raw.content.dispatch(vec![Mutation::Collapse { section: section("wrapping"), collapsed: true }]).unwrap();
        raw.frame(Vec::new(), false);
        raw.frame(Vec::new(), false);
        let reply = raw
            .content
            .dispatch(vec![
                Mutation::Collapse { section: section("wrapping"), collapsed: false },
                Mutation::Focus { instance: id("wrapping-editor") },
            ])
            .unwrap();
        raw.host.note_reply(&reply);
        raw.frame(Vec::new(), false);
        raw.frame(Vec::new(), false);
        assert_eq!(raw.ctx.memory(|memory| memory.focused()), Some(editor.0));
        assert_eq!(egui::TextEdit::load_state(&raw.ctx, editor.0).unwrap().cursor.char_range(), caret);
        assert!(trace.borrow().editors.last().unwrap().3.contains("retained selection"));
        let (invalidated, _) = raw.frame_late(vec![egui::Event::Text("!".into())], false, |_, _| {});
        assert!(invalidated.is_none(), "changed editor text invalidates its captured metrics");
        raw.frame(Vec::new(), false);
        assert_eq!(trace.borrow().editors.last().unwrap().3, expected, "typing must replace the restored selection, not insert beside it");
    }
}

#[test]
fn editor_focus_survives_floating_sizing_unless_a_live_foreign_control_takes_it() {
    for scale in [1.0, 2.0] {
        for foreign_focus in [false, true] {
            let (mut content, trace) = review_cohort_host(|_| {});
            content.dispatch(vec![fixture_open("fixture.editor", "wrapping-editor", "wrapping", Zone::Left, 0).unwrap()]).unwrap();
            let mut raw = RawPanelHost::new(content, scale);
            raw.frame(Vec::new(), false);
            raw.frame(Vec::new(), false);
            if foreign_focus {
                raw.prime_foreign_window();
            }
            let editor = trace.borrow().editors.last().unwrap().0;
            raw.ctx.memory_mut(|memory| memory.request_focus(editor));
            raw.frame(Vec::new(), false);
            raw.frame_late(vec![egui::Event::Text(" retained reparent selection".into())], false, |_, _| {});
            raw.frame(Vec::new(), false);
            let mut state = egui::TextEdit::load_state(&raw.ctx, editor).unwrap();
            state.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(1), egui::text::CCursor::new(4))));
            state.store(&raw.ctx, editor);
            raw.frame(Vec::new(), false);
            assert_eq!(raw.ctx.memory(|memory| memory.focused()), Some(editor));
            let caret = egui::TextEdit::load_state(&raw.ctx, editor).unwrap().cursor.char_range();
            assert!(!caret.unwrap().is_empty());
            let draft = trace.borrow().editors.last().unwrap().3.clone();
            let key = ControllerKey { instance: id("wrapping-editor"), owner: raw.content.runtime.instance_owner(&id("wrapping-editor")).unwrap() };
            let renders = raw.content.probe.state(&key).unwrap().renders;
            if foreign_focus {
                raw.foreign_cover = Some(egui::Rect::from_min_size(egui::pos2(1000.0, 600.0), egui::vec2(180.0, 80.0)));
                raw.foreign_takes_focus = true;
            }
            raw.content
                .dispatch(vec![Mutation::Move {
                    instance: id("wrapping-editor"),
                    destination: Destination::NewSection {
                        section: section("reparented-editor"),
                        index: 0,
                        location: SectionLocation::Floating { origin: Point { x: 400.0, y: 180.0 } },
                        size: Size { width: 400.0, height: 320.0 },
                    },
                }])
                .unwrap();
            raw.frame(Vec::new(), false);
            assert_eq!(raw.content.probe.state(&key).unwrap().renders, renders, "the new Area sizing pass must not run a hidden body");
            let foreign = foreign_focus.then(|| raw.foreign_responses.last().unwrap().id);
            assert_eq!(
                raw.ctx.memory(|memory| memory.focused()),
                foreign,
                "sizing removes the editor; only the real late foreign widget may take focus"
            );
            // Stop requesting focus: subsequent passes must retain the person's
            // choice without repeatedly forcing it after host restoration.
            raw.foreign_takes_focus = false;
            for _ in 0..2 {
                raw.frame(Vec::new(), false);
                assert_eq!(raw.ctx.memory(|memory| memory.focused()), foreign.or(Some(editor)));
            }
            assert!(raw.content.probe.state(&key).unwrap().renders > renders);
            assert_eq!(trace.borrow().editors.last().unwrap().3, draft);
            if foreign_focus {
                // A later deliberate focus command may return to the retained
                // selection. It is separate from the Move tested above.
                let reply = raw.content.dispatch(vec![Mutation::Focus { instance: id("wrapping-editor") }]).unwrap();
                raw.host.note_reply(&reply);
                raw.frame(Vec::new(), false);
                raw.frame(Vec::new(), false);
            }
            assert_eq!(raw.ctx.memory(|memory| memory.focused()), Some(editor));
            assert_eq!(egui::TextEdit::load_state(&raw.ctx, editor).unwrap().cursor.char_range(), caret);
            assert_eq!(trace.borrow().editors.last().unwrap().3, draft);
            assert_eq!(raw.content.commands.calls, 0);

            // A retained bookmark must not keep its editor focused after the
            // owning content becomes unavailable, even before another paint.
            trace.borrow_mut().unavailable.insert(key.clone());
            raw.content.tick().unwrap();
            assert_ne!(raw.content.runtime.layout().availability(&key.instance), Availability::Available);
            assert_eq!(raw.ctx.memory(|memory| memory.focused()), Some(editor));
            raw.host.reconcile_scope(&raw.content.runtime);
            assert_ne!(raw.ctx.memory(|memory| memory.focused()), Some(editor));
            assert_eq!(egui::TextEdit::load_state(&raw.ctx, editor).unwrap().cursor.char_range(), caret);
            assert_eq!(trace.borrow().editors.last().unwrap().3, draft);
        }
    }
}

#[test]
fn custom_header_action_reports_enabled_and_pressed_state_from_retained_presentation() {
    let (mut content, trace) = review_cohort_host(|_| {});
    content.dispatch(vec![fixture_open("fixture.log", "log", "log-section", Zone::Left, 0).unwrap()]).unwrap();
    let mut raw = RawPanelHost::new(content, 1.0);
    raw.frame(Vec::new(), false);
    let (frame, output) = raw.frame(Vec::new(), false);
    let target = HostControlTarget::Pointer(HostPointerTarget::HeaderAction { instance: id("log"), action: ControlId::new("pulse").unwrap() });
    let action = frame.controls.iter().find(|control| control.target == target).unwrap();
    let node = &output.platform_output.accesskit_update.as_ref().unwrap().nodes.iter().find(|(id, _)| *id == action.id.accesskit_id()).unwrap().1;
    assert_eq!(node.role(), egui::accesskit::Role::Button);
    assert_eq!(node.toggled(), Some(egui::accesskit::Toggled::True));
    assert!(!node.is_disabled());
    trace.borrow_mut().disabled = true;
    let (frame, output) = raw.frame(Vec::new(), false);
    let action = frame.controls.iter().find(|control| control.target == target).unwrap();
    assert!(!action.enabled);
    let node = &output.platform_output.accesskit_update.as_ref().unwrap().nodes.iter().find(|(id, _)| *id == action.id.accesskit_id()).unwrap().1;
    assert!(node.is_disabled());
    assert_eq!(node.toggled(), Some(egui::accesskit::Toggled::False));
}

#[test]
fn absent_content_response_cannot_restore_old_focus_but_live_response_keeps_its_caret() {
    for scale in [1.0, 2.0] {
        let (mut content, trace) = review_cohort_host(|_| {});
        content.dispatch(vec![fixture_open("fixture.editor", "wrapping-editor", "wrapping", Zone::Left, 0).unwrap()]).unwrap();
        let mut raw = RawPanelHost::new(content, scale);
        raw.frame(Vec::new(), false);
        raw.frame(Vec::new(), false);
        let editor = trace.borrow().editors.last().unwrap().clone();
        raw.ctx.memory_mut(|memory| memory.request_focus(editor.0));
        raw.frame(Vec::new(), false);
        let (invalidated, _) = raw.frame_late(vec![egui::Event::Text(" keep caret".into())], false, |_, _| {});
        assert!(invalidated.is_none(), "changed editor text invalidates its captured metrics");
        raw.frame(Vec::new(), false);
        let mut state = egui::TextEdit::load_state(&raw.ctx, editor.0).unwrap();
        state.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(1), egui::text::CCursor::new(4))));
        state.store(&raw.ctx, editor.0);
        raw.frame(Vec::new(), false);
        let caret = egui::TextEdit::load_state(&raw.ctx, editor.0).unwrap().cursor.char_range();
        trace.borrow_mut().omit_editor = true;
        let reply = raw.content.dispatch(vec![Mutation::Focus { instance: id("wrapping-editor") }]).unwrap();
        raw.host.note_reply(&reply);
        raw.frame_late(Vec::new(), false, |_, ctx| {
            assert!(ctx.read_response(editor.0).is_some(), "egui still exposes the omitted previous-pass widget");
        });
        let header = raw_control(&raw, &HostControlTarget::Pointer(HostPointerTarget::Header { instance: id("wrapping-editor") }));
        assert_eq!(raw.ctx.memory(|memory| memory.focused()), Some(header.id));
        assert_eq!(egui::TextEdit::load_state(&raw.ctx, editor.0).unwrap().cursor.char_range(), caret);
        trace.borrow_mut().omit_editor = false;
        let reply = raw.content.dispatch(vec![Mutation::Focus { instance: id("wrapping-editor") }]).unwrap();
        raw.host.note_reply(&reply);
        raw.frame(Vec::new(), false);
        assert_eq!(raw.ctx.memory(|memory| memory.focused()), Some(editor.0));
        assert_eq!(egui::TextEdit::load_state(&raw.ctx, editor.0).unwrap().cursor.char_range(), caret);
    }
}

#[test]
fn modal_dismissal_keys_preserve_passive_content_restore_but_foreign_focus_wins() {
    for scale in [1.0, 2.0] {
        for foreign in [false, true] {
            let (mut content, trace) = review_cohort_host(|_| {});
            content.dispatch(vec![fixture_open("fixture.editor", "wrapping-editor", "wrapping", Zone::Left, 0).unwrap()]).unwrap();
            let mut raw = RawPanelHost::new(content, scale);
            raw.frame(Vec::new(), false);
            raw.frame(Vec::new(), false);
            raw.prime_modal();
            let editor = trace.borrow().editors.last().unwrap().clone();
            raw.ctx.memory_mut(|memory| memory.request_focus(editor.0));
            raw.frame(Vec::new(), false);
            let (invalidated, _) = raw.frame_late(vec![egui::Event::Text(" before modal".into())], false, |_, _| {});
            assert!(invalidated.is_none(), "changed editor text invalidates its captured metrics");
            raw.frame(Vec::new(), false);
            let before = trace.borrow().editors.last().unwrap().3.clone();
            let expected: String = before.chars().take(1).chain(std::iter::once('!')).chain(before.chars().skip(4)).collect();
            let mut state = egui::TextEdit::load_state(&raw.ctx, editor.0).unwrap();
            state.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(1), egui::text::CCursor::new(4))));
            state.store(&raw.ctx, editor.0);
            raw.frame(Vec::new(), false);
            let caret = egui::TextEdit::load_state(&raw.ctx, editor.0).unwrap().cursor.char_range();
            raw.late_modal = true;
            raw.frame(Vec::new(), false);
            raw.frame(Vec::new(), false);
            raw.frame(vec![host_key(egui::Key::Escape, true)], false);
            raw.late_modal = false;
            if foreign {
                raw.foreign_cover = Some(egui::Rect::from_min_size(egui::pos2(900.0, 600.0), egui::vec2(200.0, 100.0)));
                raw.foreign_takes_focus = true;
            }
            raw.frame(vec![host_key(egui::Key::Escape, false), host_key(egui::Key::Enter, true)], false);
            raw.frame(vec![host_key(egui::Key::Enter, false)], false);
            raw.frame(Vec::new(), false);
            if foreign {
                assert_ne!(raw.ctx.memory(|memory| memory.focused()), Some(editor.0));
                raw.foreign_takes_focus = false;
                let reply = raw.content.dispatch(vec![Mutation::Focus { instance: id("wrapping-editor") }]).unwrap();
                raw.host.note_reply(&reply);
                raw.frame(Vec::new(), false);
            }
            assert_eq!(
                raw.ctx.memory(|memory| memory.focused()),
                Some(editor.0),
                "an explicit panel focus request can intentionally leave foreign focus"
            );
            assert_eq!(egui::TextEdit::load_state(&raw.ctx, editor.0).unwrap().cursor.char_range(), caret);
            assert!(!caret.unwrap().is_empty());
            let (invalidated, _) = raw.frame_late(vec![egui::Event::Text("!".into())], false, |_, _| {});
            assert!(invalidated.is_none(), "changed editor text invalidates its captured metrics");
            raw.frame(Vec::new(), false);
            assert_eq!(trace.borrow().editors.last().unwrap().3, expected, "modal return must retain replacement selection");
        }
    }
}

#[test]
fn late_modal_reentry_surrenders_content_focused_after_mount() {
    for scale in [1.0, 2.0] {
        let (mut content, trace) = review_cohort_host(|_| {});
        content.dispatch(vec![fixture_open("fixture.editor", "wrapping-editor", "wrapping", Zone::Left, 0).unwrap()]).unwrap();
        let mut raw = RawPanelHost::new(content, scale);
        raw.frame(Vec::new(), false);
        raw.frame(Vec::new(), false);
        raw.prime_modal();
        let editor = trace.borrow().editors.last().unwrap().clone();
        raw.ctx.memory_mut(|memory| memory.request_focus(editor.0));
        raw.frame(Vec::new(), false);
        let caret = egui::TextEdit::load_state(&raw.ctx, editor.0).unwrap().cursor.char_range();
        raw.late_modal = true;
        raw.frame(Vec::new(), false);
        assert_ne!(raw.ctx.memory(|memory| memory.focused()), Some(editor.0));
        raw.late_modal = false;
        raw.frame(Vec::new(), false);
        let reply = raw.content.dispatch(vec![Mutation::Focus { instance: id("wrapping-editor") }]).unwrap();
        raw.host.note_reply(&reply);
        raw.late_modal = true;
        raw.frame_late(Vec::new(), false, |_, ctx| {
            assert_eq!(
                ctx.memory(|memory| memory.focused()),
                Some(editor.0),
                "the actual content target receives focus after mount before the late modal is finalized"
            );
        });
        assert_ne!(raw.ctx.memory(|memory| memory.focused()), Some(editor.0), "the late modal must surrender newly restored content focus too");
        assert_eq!(egui::TextEdit::load_state(&raw.ctx, editor.0).unwrap().cursor.char_range(), caret);
        assert_eq!(raw.content.commands.calls, 0);
    }
}

fn production_content_harness(combined_history: bool, assistant: bool) -> crate::scenario::Harness {
    let mut h = crate::scenario::Harness::new();
    h.app.home.open = false;
    h.app.ui.show_browser = true;
    h.app.ui.show_timeline = true;
    h.app.preferences.timeline_left = false;
    h.app.preferences.history_in_browser = combined_history;
    h.app.preferences.ai_features = assistant;
    h.app.agent_chat.open = assistant;
    for name in ["First", "Second"] {
        h.app.run("sketch.create", serde_json::json!({"plane":"XY","name":name})).unwrap();
        h.app.run("sketch.finish", serde_json::json!({})).unwrap();
    }
    h.app.disable_panel_runtime().unwrap();
    // These fixtures seed first-run settings, without restoring the harness's earlier layout.
    h.app.panel_layout_preferences = Default::default();
    super::production::enable_production(&mut h.app).unwrap();
    h.frames(3);
    h
}

fn production_controller_keys(app: &crate::SolveApp) -> Vec<ControllerKey> {
    let runtime = app.panel_runtime().unwrap();
    ["browser", "history", "assistant", "render-browser"]
        .into_iter()
        .filter_map(|name| {
            let instance = id(name);
            let owner = runtime.instance_owner(&instance).unwrap();
            let key = ControllerKey { instance, owner };
            runtime.state(&key).map(|_| key)
        })
        .collect()
}

#[test]
fn production_browser_and_history_mount_existing_content_once_and_preserve_combined_history() {
    for combined in [false, true] {
        let h = production_content_harness(combined, false);
        let frame = h.app.universal_panel_frame().expect("real content uses the shared host");
        for card in &frame.layout.sections {
            assert_eq!(card.surface, PanelSurface::Translucent, "production panels follow the ribbon in every in-workspace zone");
        }
        assert!(frame.layout.sections.iter().any(|card| card.instance == id("browser")));
        assert_eq!(frame.layout.sections.iter().any(|card| card.instance == id("history")), !combined);
        let rows = crate::browser::drawn_rows();
        for name in ["First", "Second"] {
            assert_eq!(rows.iter().filter(|row| row.0 == name).count(), 1, "{name} has one canonical Browser presentation");
            assert_eq!(crate::scenario::handle_at(&format!("timeline:{name}")).is_some(), !combined);
        }
        if combined {
            assert!(rows.iter().any(|row| row.0 == solidcraft_ui_text::tr("ui.history_title")), "combined History keeps its real Browser folder");
        } else {
            let history = frame.layout.sections.iter().find(|card| card.instance == id("history")).unwrap();
            assert!(history.inline, "Bottom History uses the generic one-row composition");
            let body = egui_rect(history.inline_content.unwrap());
            let first = crate::scenario::handle_at("timeline:First").unwrap();
            assert!(body.contains(first), "the real feature chips are inside the shared inline slot");
        }
        assert!(h.app.panel_runtime().unwrap().update_errors().is_empty());
    }
}

#[test]
fn production_tab_collapse_and_move_keep_real_selection_document_camera_and_retained_owners() {
    let mut h = production_content_harness(false, false);
    let feature = h.app.session.doc.features[0].id;
    h.app.run("select.set", serde_json::json!({"items":[solidcraft_engine::Sel::Feature { id: feature }]})).unwrap();
    h.frames(2);
    let session = h.app.session.uid;
    let document = h.app.session.doc.clone();
    let model = h.app.session.model.state();
    let camera = h.app.cam;
    let selection = h.app.session.selection.clone();
    let revision = h.app.session.revision;
    let undo = h.app.session.undo.len();
    let dirty = h.app.session.is_dirty();
    let keys = production_controller_keys(&h.app);
    let controllers = h.app.panel_runtime().unwrap().controller_count();
    let browser_section = h.app.panel_runtime().unwrap().layout().layout().containing(&id("browser")).unwrap().id.clone();
    h.app
        .dispatch_panel_layout(transaction(vec![
            Mutation::Move { instance: id("history"), destination: Destination::JoinTab { section: browser_section.clone(), index: 1 } },
            Mutation::Activate { instance: id("history") },
        ]))
        .unwrap();
    h.frames(3);
    assert!(crate::browser::drawn_rows().is_empty(), "an inactive Browser does not render a hidden body");
    assert!(crate::scenario::handle_at("timeline:First").is_some(), "History switches to its existing side rows");
    h.app.dispatch_panel_layout(transaction(vec![Mutation::Activate { instance: id("browser") }])).unwrap();
    h.frames(2);
    assert!(crate::browser::drawn_rows().iter().any(|row| row.0 == "First"));
    for collapsed in [true, false] {
        h.app.dispatch_panel_layout(transaction(vec![Mutation::Collapse { section: browser_section.clone(), collapsed }])).unwrap();
        h.frames(2);
        assert_eq!(crate::browser::drawn_rows().is_empty(), collapsed, "collapse controls real body work");
    }
    h.app
        .dispatch_panel_layout(transaction(vec![Mutation::Move {
            instance: id("browser"),
            destination: Destination::NewSection {
                section: section("moved-browser"),
                index: 0,
                location: SectionLocation::Dock { zone: Zone::Right },
                size: Size { width: 300.0, height: 320.0 },
            },
        }]))
        .unwrap();
    h.frames(3);
    let card = h.app.universal_panel_frame().unwrap().layout.sections.iter().find(|card| card.instance == id("browser")).unwrap();
    assert_eq!(card.zone, Zone::Right);
    assert!(crate::browser::drawn_rows().iter().any(|row| row.0 == "First"));
    assert_eq!(production_controller_keys(&h.app), keys);
    assert_eq!(h.app.panel_runtime().unwrap().controller_count(), controllers);
    assert_eq!(h.app.session.uid, session);
    assert!(std::sync::Arc::ptr_eq(&h.app.session.doc, &document));
    assert!(std::sync::Arc::ptr_eq(&h.app.session.model.state(), &model));
    assert_eq!(h.app.cam, camera);
    assert_eq!(h.app.session.selection, selection);
    assert_eq!(h.app.session.revision, revision);
    assert_eq!(h.app.session.undo.len(), undo);
    assert_eq!(h.app.session.is_dirty(), dirty);
}

#[test]
fn production_browser_and_assistant_refuse_bottom_without_changing_live_content() {
    let mut h = production_content_harness(false, true);
    h.app.agent_chat.input = "keep this unsent draft".into();
    let document = h.app.session.doc.clone();
    let camera = h.app.cam;
    let controllers = h.app.panel_runtime().unwrap().controller_count();
    for instance in ["browser", "assistant"] {
        let before = h.app.panel_runtime().unwrap().layout().inspect();
        let result = h.app.dispatch_panel_layout(transaction(vec![Mutation::Move {
            instance: id(instance),
            destination: Destination::NewSection {
                section: section("forbidden-bottom"),
                index: 0,
                location: SectionLocation::Dock { zone: Zone::Bottom },
                size: Size { width: 400.0, height: 120.0 },
            },
        }]));
        assert_eq!(result.unwrap_err().rule, Rule::Placement, "only product History permits Bottom");
        assert_eq!(h.app.panel_runtime().unwrap().layout().inspect(), before);
        h.frames(2);
        assert_eq!(h.app.agent_chat.input, "keep this unsent draft");
        assert!(std::sync::Arc::ptr_eq(&h.app.session.doc, &document));
        assert_eq!(h.app.cam, camera);
        assert_eq!(h.app.panel_runtime().unwrap().controller_count(), controllers);
    }
}

#[test]
fn production_host_ignores_the_retired_classic_switch() {
    let mut h = crate::scenario::Harness::new();
    let mut saved: serde_json::Value = serde_json::from_str(&h.app.prefs()).unwrap();
    saved["preferences"]["shared_panel_layout"] = serde_json::json!(false);
    h.app.disable_panel_runtime().unwrap();
    h.app.load_prefs(&saved.to_string());
    h.app.home.open = false;
    h.frames(3);
    assert!(h.app.universal_panel_frame().is_some());
    assert!(!h.app.prefs().contains("shared_panel_layout"));
}

#[test]
fn production_preferences_without_a_panel_record_preserve_the_live_catalogue() {
    let mut h = production_content_harness(false, false);
    let mut saved: serde_json::Value = serde_json::from_str(&h.app.prefs()).unwrap();
    saved.as_object_mut().unwrap().remove(super::preferences::PREFERENCES_KEY);
    let current = super::preferences::preferences_value(&h.app).unwrap();
    h.app.load_prefs(&saved.to_string());
    h.frames(2);
    assert_eq!(super::preferences::preferences_value(&h.app).unwrap(), current);
    assert!(super::preferences::preferences_enabled(&h.app));
}

#[test]
fn production_assistant_actions_use_the_existing_confirmation_and_close_retains_its_draft_and_queue() {
    let mut h = production_content_harness(false, true);
    h.app.agent_chat.input = "unsent composer draft".into();
    h.app.agent_chat.items.push(crate::agent_chat::Item::Agent { thought: false, text: "retained transcript".into() });
    h.app.agent_chat.pending.push(crate::agent_chat::Pending {
        text: "queued instruction".into(),
        image: false,
        context: Default::default(),
        files: Vec::new(),
    });
    let pending = h.app.agent_chat.pending.clone();
    let state = h.app.agent_chat.state();
    let keys = production_controller_keys(&h.app);
    let controllers = h.app.panel_runtime().unwrap().controller_count();
    h.app.panel_content_action(&id("assistant"), &ControlId::new("new-chat").unwrap()).unwrap();
    super::production::drain_production_actions(&mut h.app).unwrap();
    assert!(h.app.agent_chat.confirm_new, "the real New Chat confirmation protects queued instructions");
    assert_eq!(h.app.agent_chat.items.len(), 1);
    assert_eq!(h.app.agent_chat.pending, pending);
    h.app.agent_chat.confirm_new = false;
    h.frames(2);
    h.app.dispatch_panel_layout(transaction(vec![Mutation::Close { instance: id("assistant") }])).unwrap();
    h.frames(2);
    assert!(!h.app.universal_panel_frame().unwrap().layout.sections.iter().any(|card| card.instance == id("assistant")));
    assert_eq!(h.app.agent_chat.input, "unsent composer draft");
    assert_eq!(h.app.agent_chat.pending, pending);
    assert_eq!(h.app.agent_chat.items.len(), 1);
    assert_eq!(h.app.agent_chat.state(), state, "closing presentation does not restart or cancel the retained chat service");
    h.app.start("ui.assistant");
    h.frames(2);
    assert!(h.app.agent_chat.panel_rect.is_some(), "the existing Assistant button reopens the shared body after quick close");
    let section = h.app.panel_runtime().unwrap().layout().layout().containing(&id("assistant")).unwrap().id.clone();
    h.app.dispatch_panel_layout(transaction(vec![Mutation::Collapse { section: section.clone(), collapsed: true }])).unwrap();
    h.frames(2);
    assert!(h.app.agent_chat.panel_rect.is_none());
    crate::agent_chat::toggle(&mut h.app);
    h.frames(2);
    assert!(h.app.agent_chat.panel_rect.is_some(), "the keyboard toggle expands a shared collapsed Assistant");
    h.app.dispatch_panel_layout(transaction(vec![Mutation::Collapse { section, collapsed: true }])).unwrap();
    h.app.dispatch_panel_layout(transaction(vec![Mutation::Close { instance: id("assistant") }])).unwrap();
    h.frames(2);
    h.app.start("ui.assistant");
    h.frames(2);
    assert!(h.app.agent_chat.panel_rect.is_some(), "opening a panel remembered as collapsed expands it atomically");
    h.app.start("ui.assistant");
    h.frames(2);
    assert!(h.app.agent_chat.panel_rect.is_none(), "the existing Assistant button closes an expanded shared Assistant");
    crate::agent_chat::toggle(&mut h.app);
    h.frames(2);
    assert!(h.app.agent_chat.panel_rect.is_some());
    assert_eq!(h.app.agent_chat.input, "unsent composer draft");
    assert_eq!(h.app.agent_chat.pending, pending);
    assert_eq!(production_controller_keys(&h.app), keys);
    assert_eq!(h.app.panel_runtime().unwrap().controller_count(), controllers);
    h.app.agent_chat.pending.clear();
    h.app.panel_content_action(&id("assistant"), &ControlId::new("new-chat").unwrap()).unwrap();
    super::production::drain_production_actions(&mut h.app).unwrap();
    assert!(!h.app.agent_chat.confirm_new);
    assert!(h.app.agent_chat.items.is_empty(), "the same New Chat action clears the real transcript when no confirmation is needed");
    assert_eq!(h.app.agent_chat.input, "unsent composer draft");
}

#[test]
fn production_render_browser_reuses_real_lens_search_and_retains_explicit_layout_workspace() {
    let mut h = production_content_harness(false, false);
    let workspace = h.app.panel_runtime().unwrap().layout().layout().workspace.clone();
    let document = h.app.session.doc.clone();
    let camera = h.app.cam;
    h.app.ui.tab = "RENDER".into();
    h.frames(3);
    let frame = h.app.universal_panel_frame().unwrap();
    assert!(frame.layout.sections.iter().any(|card| card.instance == id("render-browser")));
    assert!(!frame.layout.sections.iter().any(|card| card.instance == id("browser")));
    h.app.panel_content_action(&id("render-browser"), &ControlId::new("by-look").unwrap()).unwrap();
    super::production::drain_production_actions(&mut h.app).unwrap();
    h.frames(2);
    assert!(h.app.render_workspace.view.by_look);
    h.app.panel_content_action(&id("render-browser"), &ControlId::new("search").unwrap()).unwrap();
    super::production::drain_production_actions(&mut h.app).unwrap();
    h.frames(2);
    assert!(h.app.render_workspace.view.search_objects);
    let search = h.app.panel_runtime().unwrap().instance_presentation(&id("render-browser")).unwrap();
    assert_eq!(search.actions.iter().find(|action| action.id.0 == "search").unwrap().pressed, Some(true));
    assert_eq!(h.app.panel_runtime().unwrap().layout().layout().workspace, workspace, "changing ribbon tabs never selects a layout workspace");
    assert!(std::sync::Arc::ptr_eq(&h.app.session.doc, &document));
    assert_eq!(h.app.cam, camera);
    h.app.ui.tab = "SOLID".into();
    h.frames(2);
    assert!(h.app.universal_panel_frame().unwrap().layout.sections.iter().any(|card| card.instance == id("browser")));
    h.app.ui.tab = "RENDER".into();
    h.frames(2);
    assert!(h.app.render_workspace.view.by_look && h.app.render_workspace.view.search_objects);
}

#[test]
fn production_render_panels_reopen_independently_with_existing_controls() {
    let mut h = production_content_harness(false, false);
    // Restore a layout saved before the three Render panels were registered.
    let names = ["render-library", "render-inspector", "render-shots"];
    let mut old = h.app.panel_runtime().unwrap().layout().layout().clone();
    old.instances.retain(|instance| !names.contains(&instance.id.0.as_str()));
    old.sections.retain(|section| !names.contains(&section.active.0.as_str()));
    old.columns.retain(|column| old.sections.iter().any(|section| section.column.as_ref() == Some(&column.id)));
    h.app.dispatch_panel_layout(LayoutAction::Replace { expected_revision: None, layout: old }).unwrap();

    let document = h.app.session.doc.clone();
    h.app.ui.tab = "RENDER".into();
    h.frames(12);
    let presented = |app: &crate::SolveApp, name| {
        app.universal_panel_frame()
            .unwrap()
            .layout
            .sections
            .iter()
            .any(|card| card.instance == id(name) && (card.body.is_some() || card.inline_content.is_some()))
    };
    for name in ["render-browser", "render-library", "render-inspector", "render-shots"] {
        assert!(presented(&h.app, name), "old layout should present {name}");
    }
    let frame = h.app.universal_panel_frame().unwrap();
    let shots = frame.layout.sections.iter().find(|card| card.instance == id("render-shots")).unwrap();
    assert_eq!(shots.zone, Zone::Bottom, "older layouts gain Shots in the Bottom dock");
    assert!(shots.inline && shots.body.is_none(), "Bottom Shots fits in one shared row");
    assert!(crate::scenario::rect_of("render:editor:scene").is_some());
    assert!(crate::scenario::rect_of("render:shots:status").is_some());
    assert!(!h.app.shell.workspace_panels.contains_key("render:appearances"));
    assert!(!h.app.shell.workspace_panels.contains_key("render:inspector"));
    assert!(!h.app.shell.workspace_panels.contains_key("render:shots_strip"));

    h.app
        .dispatch_panel_layout(transaction(vec![
            Mutation::Close { instance: id("render-library") },
            Mutation::Close { instance: id("render-browser") },
        ]))
        .unwrap();
    h.frames(3);
    assert!(!presented(&h.app, "render-library"));
    assert!(presented(&h.app, "render-inspector"));
    assert!(presented(&h.app, "render-shots"));
    h.app.render_workspace.view.library_focus_pending = true;
    h.frames(3);
    assert!(presented(&h.app, "render-library"));
    assert!(!presented(&h.app, "render-browser"));
    assert!(std::sync::Arc::ptr_eq(&h.app.session.doc, &document));
}

#[test]
fn production_short_render_assistant_keeps_conversation_above_the_pinned_composer() {
    let mut h = production_content_harness(false, true);
    h.call("ui.resize", serde_json::json!({"width":1600,"height":480}));
    h.app.ui.tab = "RENDER".into();
    for draft in [String::new(), "Retained panel draft\n".repeat(40)] {
        h.app.agent_chat.input = draft.clone();
        h.frames(8);
        let conversation = crate::scenario::rect_of("agent_chat:conversation").unwrap();
        let composer = crate::scenario::rect_of("agent_chat:composer_footer").unwrap();
        assert!(conversation.height() < 64.0, "exercise the short viewport that formerly expanded over the composer: {conversation:?}");
        assert!(
            !conversation.is_positive() || conversation.bottom() <= composer.top() + 1.0,
            "a short transcript viewport must not paint or scroll over the composer: draft bytes={}, conversation={conversation:?}, composer={composer:?}",
            draft.len()
        );
        let body = h.app.universal_panel_frame().unwrap().layout.sections.iter().find(|card| card.instance == id("assistant")).unwrap().body.unwrap();
        assert!(composer.bottom() <= body.origin.y + body.size.height + 1.0, "the full footer remains inside the shared content slot");
        assert!(crate::scenario::rect_of("agent_chat:input").is_some());
        assert_eq!(h.app.agent_chat.input, draft);
    }
}

#[test]
fn production_sole_content_tracks_real_tree_folds_and_stops_at_column_capacity() {
    let mut h = production_content_harness(false, true);
    h.frames(6);
    let frame = h.app.universal_panel_frame().unwrap();
    let browser = frame.layout.sections.iter().find(|card| card.instance == id("browser")).unwrap();
    let original_height = browser.outer.size.height;
    assert!(original_height < frame.layout.capacity.size.height - 20.0);
    let assistant = frame.layout.sections.iter().find(|card| card.instance == id("assistant")).unwrap();
    let history = frame.layout.sections.iter().find(|card| card.instance == id("history")).unwrap();
    let available = history.outer.origin.y - frame.layout.metrics.gap - assistant.outer.origin.y;
    assert!((assistant.outer.size.height - available).abs() < 1.0, "Assistant fills its available column height even with an empty conversation");
    let root = crate::browser::drawn_rows().first().unwrap().0.clone();
    let point = h.call("ui.at", serde_json::json!({"chevron":root}));
    h.call("ui.click", serde_json::json!({"x":point["result"][0],"y":point["result"][1]}));
    h.frames(6);
    let folded = h.app.universal_panel_frame().unwrap().layout.sections.iter().find(|card| card.instance == id("browser")).unwrap().outer.size.height;
    assert!(folded < original_height - 20.0, "folding real content releases the unused space");
    let point = h.call("ui.at", serde_json::json!({"chevron":root}));
    h.call("ui.click", serde_json::json!({"x":point["result"][0],"y":point["result"][1]}));
    h.call("ui.resize", serde_json::json!({"width":1600,"height":350}));
    h.frames(6);
    let frame = h.app.universal_panel_frame().unwrap();
    let browser = frame.layout.sections.iter().find(|card| card.instance == id("browser")).unwrap();
    let history = frame.layout.sections.iter().find(|card| card.instance == id("history")).unwrap();
    let available = history.outer.origin.y - frame.layout.metrics.gap - browser.outer.origin.y;
    assert!((browser.outer.size.height - available).abs() < 1.0, "long content is capped above Bottom History and its gap");
}

#[test]
fn production_bottom_history_has_fixed_height_without_collapse_or_height_changes() {
    let mut h = production_content_harness(false, false);
    h.frames(6);
    let frame = h.app.universal_panel_frame().unwrap();
    let history = frame.layout.sections.iter().find(|card| card.instance == id("history")).unwrap();
    let spec = h.app.panel_runtime().unwrap().layout().registry().get(&PanelKind::new("solidcraft.history").unwrap()).unwrap();
    assert_eq!(history.outer.size.height, spec.fixed_bottom_height.unwrap());
    assert!(history.collapse.is_none());
    let before = h.app.panel_runtime().unwrap().layout().inspect();
    assert!(h.app.run_panel_command("panel.collapse", &serde_json::json!({"target":{"type":"instance","id":"history"},"collapsed":true})).is_err());
    assert!(
        h.app
            .run_panel_command(
                "panel.resize",
                &serde_json::json!({"target":{"type":"instance","id":"history"},"size":{"width":760,"height":100},"thickness_only":true})
            )
            .is_err()
    );
    assert_eq!(h.app.panel_runtime().unwrap().layout().inspect(), before);
}

#[test]
fn production_history_play_preference_updates_the_visible_shared_controls() {
    let mut h = production_content_harness(false, false);
    let play_is_visible = |app: &crate::SolveApp| {
        app.universal_panel_frame()
            .unwrap()
            .layout
            .sections
            .iter()
            .any(|section| section.instance == id("history") && section.actions.iter().any(|(action, _)| action.0 == "play"))
    };
    assert!(play_is_visible(&h.app));
    h.app.preferences.history_play_button = false;
    h.frames(2);
    assert!(!play_is_visible(&h.app));
    h.app.preferences.history_play_button = true;
    h.frames(2);
    assert!(play_is_visible(&h.app));
}

#[test]
fn production_history_compatibility_command_updates_the_shared_layout() {
    let mut h = production_content_harness(false, false);
    h.app.run("view.history_layout", serde_json::json!({"dock":"left","width":340.0,"collapsed":true})).unwrap();
    let layout = h.app.panel_runtime().unwrap().layout().layout();
    let history = layout.containing(&id("history")).unwrap();
    assert_eq!(history.location.zone(), Zone::Left);
    assert!(history.collapsed);
    assert_eq!(layout.column(history.column.as_ref().unwrap()).unwrap().width, 340.0);
    let browser = layout.containing(&id("browser")).unwrap();
    assert_ne!(history.column, browser.column);
    assert_eq!(layout.column(browser.column.as_ref().unwrap()).unwrap().width, crate::prefs::DEFAULT_BROWSER_WIDTH);

    h.app.run("view.history_layout", serde_json::json!({"dock":"bottom"})).unwrap();
    let history = h.app.panel_runtime().unwrap().layout().layout().containing(&id("history")).unwrap();
    assert_eq!(history.location.zone(), Zone::Bottom);
    assert!(!history.collapsed);
}

#[test]
fn production_fixed_bottom_history_ignores_a_legacy_collapsed_preference() {
    let mut h = crate::scenario::Harness::new();
    h.app.home.open = false;
    h.app.ui.show_timeline = true;
    h.app.preferences.timeline_left = false;
    h.app.preferences.timeline_collapsed = true;
    h.app.disable_panel_runtime().unwrap();
    // These fixtures seed first-run settings, without restoring the harness's earlier layout.
    h.app.panel_layout_preferences = Default::default();
    super::production::enable_production(&mut h.app).unwrap();
    h.frames(6);
    let section = h.app.panel_runtime().unwrap().layout().layout().containing(&id("history")).unwrap();
    assert_eq!(section.location.zone(), Zone::Bottom);
    assert!(!section.collapsed);
}

#[test]
fn universal_chrome_controls_have_real_accessible_widgets_and_stable_ids_at_both_scales() {
    use crate::panel_runtime::{HostControlTarget, HostPointerTarget, enable_host_specimen};
    for scale in [1.0, 2.0] {
        for preset in ["docks", "tabs", "collapsed", "overflow", "floating", "inline"] {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            h.ctx.set_pixels_per_point(scale);
            h.call("ui.resize", serde_json::json!({"width":800,"height":600}));
            enable_host_specimen(&mut h.app, preset).unwrap();
            h.frames(3);
            let before = h.app.universal_panel_frame().unwrap().controls.clone();
            assert!(!before.is_empty(), "actual chrome at scale {scale}: {preset}");
            let tree = h.output.accesskit_update.as_ref().expect("real egui accessibility update");
            let mut ids = std::collections::HashSet::new();
            for control in &before {
                assert!(ids.insert(control.id), "semantic IDs cannot collide: {control:?}");
                assert!(!control.accessible_name.is_empty());
                control.bounds.validate().unwrap();
                let interactive = !matches!(&control.target, HostControlTarget::Pointer(HostPointerTarget::Surface { .. }));
                if interactive && control.enabled {
                    let node = tree
                        .nodes
                        .iter()
                        .find(|(id, _)| *id == control.id.accesskit_id())
                        .map(|(_, node)| node)
                        .unwrap_or_else(|| panic!("exported control needs its own actual widget: {control:?}"));
                    assert_eq!(node.label(), Some(control.accessible_name.as_str()), "actual widget/metadata name parity: {control:?}");
                    let expected_role = match &control.target {
                        HostControlTarget::Pointer(HostPointerTarget::Tab { .. }) => egui::accesskit::Role::Tab,
                        HostControlTarget::Pointer(HostPointerTarget::Sash { .. } | HostPointerTarget::Resize { .. }) => {
                            egui::accesskit::Role::Splitter
                        }
                        HostControlTarget::Scroll { .. } => egui::accesskit::Role::ScrollBar,
                        HostControlTarget::PanelMenu { .. } | HostControlTarget::Shared { .. } => egui::accesskit::Role::MenuItem,
                        HostControlTarget::Pointer(_) | HostControlTarget::Content { .. } => egui::accesskit::Role::Button,
                    };
                    assert_eq!(node.role(), expected_role, "the actual widget role follows its typed target: {control:?}");
                    if matches!(&control.target, HostControlTarget::Pointer(HostPointerTarget::Menu { .. })) {
                        assert_eq!(control.accessible_name, solidcraft_ui_text::tr("ui.panel_host.menu"));
                    }
                }
            }
            h.frames(2);
            let after = &h.app.universal_panel_frame().unwrap().controls;
            for control in before {
                let retained = after.iter().find(|other| other.target == control.target).expect("same semantic target on the next frame");
                assert_eq!(retained.id, control.id, "status/service update cannot change widget identity");
                assert_eq!(retained.bounds, control.bounds);
            }
        }
    }
}

#[test]
fn universal_host_slots_and_menu_button_stay_bounded_at_minimum_and_small_window_sizes() {
    use crate::panel_runtime::enable_host_specimen;
    for scale in [1.0, 2.0] {
        for (width, height) in [(1280, 800), (800, 600), (480, 320)] {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            h.ctx.set_pixels_per_point(scale);
            h.call("ui.resize", serde_json::json!({"width":width,"height":height}));
            enable_host_specimen(&mut h.app, "tabs").unwrap();
            h.frames(3);
            let host = &h.app.universal_panel_frame().unwrap().layout;
            host.resolved.validate().unwrap();
            for section in &host.sections {
                assert!(section.outer.contains_rect(section.header));
                assert!(section.header.contains_rect(section.menu));
                if let Some(body) = section.body {
                    assert!(section.outer.contains_rect(body));
                    assert!(body.origin.y >= section.header.origin.y + section.header.size.height);
                }
                assert_eq!(section.header.size.height, host.metrics.header_height);
                if let Some(strip) = section.tab_strip {
                    assert_eq!(strip.origin.y, section.header.origin.y);
                    assert_eq!(strip.size.height, section.header.size.height);
                    assert!(section.header.contains_rect(strip));
                    assert!(section.outer.contains_rect(strip));
                    if let Some(body) = section.body {
                        assert!(body.origin.y >= strip.origin.y + strip.size.height);
                    }
                }
                if let Some(footer) = section.footer {
                    assert!(section.outer.contains_rect(footer));
                    if let Some(body) = section.body {
                        assert_eq!(footer.origin.y, body.origin.y + body.size.height);
                    }
                    assert_eq!(footer.size.height, host.metrics.footer_height);
                }
                assert!(section.menu.size.width > 0.0 && section.menu.size.height > 0.0, "shared actions remain reachable when content fits");
            }
            for overflow in &host.overflow {
                assert!(!overflow.sections.is_empty());
                if let Some(bounds) = overflow.bounds {
                    assert!(host.capacity.contains_rect(bounds));
                }
            }
        }
    }
}

#[test]
fn universal_collapsed_header_keeps_actions_and_one_click_restores_the_same_controller() {
    use crate::panel_runtime::{HostControlTarget, HostPointerTarget, enable_host_specimen, inspect_host_specimen};
    let mut h = crate::scenario::Harness::new();
    h.app.home.open = false;
    h.call("ui.resize", serde_json::json!({"width":1400,"height":900}));
    enable_host_specimen(&mut h.app, "collapsed").unwrap();
    h.frames(3);
    let before = inspect_host_specimen(&mut h.app).unwrap();
    let frame = h.app.universal_panel_frame().unwrap();
    assert!(frame.layout.sections.iter().all(|section| section.collapsed && section.body.is_none() && section.footer.is_none()));
    assert!(frame.controls.iter().any(|control| matches!(&control.target,
        HostControlTarget::Pointer(HostPointerTarget::HeaderAction { instance, action }) if instance.0 == "specimen-0" && action.0 == "apply")
        && control.enabled));
    assert!(frame.controls.iter().any(|control| matches!(&control.target,
        HostControlTarget::Pointer(HostPointerTarget::Close { instance }) if instance.0 == "specimen-0")));
    assert!(!frame.controls.iter().any(|control| matches!(&control.target,
        HostControlTarget::Pointer(HostPointerTarget::Close { instance }) if instance.0 == "specimen-1")));
    let expand = frame
        .controls
        .iter()
        .find(|control| {
            matches!(&control.target,
        HostControlTarget::Pointer(HostPointerTarget::Collapse { instance }) if instance.0 == "specimen-0")
        })
        .unwrap();
    let at = crate::panel_runtime::egui_rect(expand.bounds).center();
    h.call("ui.click", serde_json::json!({"x":at.x,"y":at.y}));
    h.frames(2);
    let restored = h.app.universal_panel_frame().unwrap().layout.sections.iter().find(|section| section.instance.0 == "specimen-0").unwrap();
    assert!(!restored.collapsed);
    assert!(restored.body.is_some());
    let after = inspect_host_specimen(&mut h.app).unwrap();
    let controller = |value: &serde_json::Value| {
        value["runtime"]["probe"]["controllers"].as_array().unwrap().iter().find(|state| state["instance"] == "specimen-0").unwrap().clone()
    };
    let old = controller(&before);
    let new = controller(&after);
    assert_eq!(old["renders"], 0);
    assert!(new["renders"].as_u64().unwrap() > 0);
    assert_eq!(old["controller_identity"], new["controller_identity"]);
    assert_eq!(old["cancels"], new["cancels"]);
    assert_eq!(old["draft"], new["draft"]);
    assert_eq!(before["session"]["camera"], after["session"]["camera"]);
}

#[test]
fn universal_sole_content_gap_is_navigable_while_declared_fill_uses_the_column() {
    use crate::panel_runtime::{enable_host_specimen, logical_rect};
    let mut h = crate::scenario::Harness::new();
    h.app.home.open = false;
    h.call("ui.resize", serde_json::json!({"width":1400,"height":900}));
    enable_host_specimen(&mut h.app, "content_fill").unwrap();
    h.frames(3);
    let frame = h.app.universal_panel_frame().unwrap();
    let content = frame.layout.sections.iter().find(|section| section.instance.0 == "specimen-0").unwrap();
    let fill = frame.layout.sections.iter().find(|section| section.instance.0 == "specimen-1").unwrap();
    assert!(fill.outer.size.height > content.outer.size.height);
    let outer = crate::panel_runtime::egui_rect(content.outer);
    let gap = egui::Rect::from_center_size(egui::pos2(outer.center().x, outer.bottom() + 20.0), egui::vec2(4.0, 4.0));
    assert!(frame.layout.usable.contains_rect(logical_rect(gap)));
    assert!(!frame.layout.consumes_pointer(solidcraft_engine::panels::Point { x: gap.center().x, y: gap.center().y }));
    assert!(frame.layout.resolved.rails.is_empty() && frame.layout.overflow.is_empty());
    let original = fill.outer.size.height;
    let target = HostControlTarget::Pointer(HostPointerTarget::Resize { section: fill.section.clone(), edge: HostResizeEdge::Bottom });
    let base = egui_rect(frame.controls.iter().find(|control| control.target == target).unwrap().bounds).center();
    let result = h.call("ui.drag", serde_json::json!({"x0":base.x,"y0":base.y,"x1":base.x,"y1":base.y-100.0,"steps":8}));
    assert_eq!(result["ok"], true);
    h.frames(3);
    let fill = h.app.universal_panel_frame().unwrap().layout.sections.iter().find(|section| section.instance.0 == "specimen-1").unwrap();
    assert_eq!(fill.outer.size.height, original - 100.0);
}

#[test]
fn universal_inline_specimen_uses_one_total_row_with_real_content_and_shared_controls() {
    use crate::panel_runtime::{HostControlTarget, HostPointerTarget, enable_host_specimen};
    let mut h = crate::scenario::Harness::new();
    h.app.home.open = false;
    h.call("ui.resize", serde_json::json!({"width":1400,"height":900}));
    enable_host_specimen(&mut h.app, "inline").unwrap();
    h.frames(3);
    let frame = h.app.universal_panel_frame().unwrap();
    let section = frame.layout.sections.first().unwrap();
    assert!(section.inline && !section.collapsed);
    assert_eq!(section.header, section.outer);
    assert!(section.outer.size.height <= solidcraft_theme::PANEL_INLINE_MAX_HEIGHT);
    assert!(section.body.is_none() && section.footer.is_none());
    assert!(!frame.controls.iter().any(|control| matches!(control.target, HostControlTarget::Pointer(HostPointerTarget::HeaderAction { .. }))));
    let runtime = h.app.panel_runtime().unwrap();
    let instance = runtime.layout().layout().instance(&section.instance).unwrap();
    assert!(runtime.layout().registry().get(&instance.kind).unwrap().header_actions.is_empty(), "the inline log declares no custom or debug actions");
    assert!(frame.controls.iter().any(|control| matches!(control.target, HostControlTarget::Pointer(HostPointerTarget::Menu { .. }))));
    assert!(frame.controls.iter().any(|control| matches!(control.target, HostControlTarget::Pointer(HostPointerTarget::Collapse { .. }))));
    assert!(h.widget(solidcraft_ui_text::tr("ui.panel_fixture.inline")).is_some());
    assert!(h.widget(solidcraft_ui_text::tr("ui.panel_fixture.caption")).is_none());
}

#[test]
fn universal_registered_header_command_invokes_only_the_declared_synthetic_binding() {
    use crate::panel_runtime::{enable_host_specimen, inspect_host_specimen};
    let mut h = crate::scenario::Harness::new();
    h.app.home.open = false;
    h.call("ui.resize", serde_json::json!({"width":1400,"height":900}));
    enable_host_specimen(&mut h.app, "collapsed").unwrap();
    h.frames(3);
    let before = inspect_host_specimen(&mut h.app).unwrap();
    h.app.run_panel_command("panel.action", &serde_json::json!({"target":{"type":"instance","id":"specimen-0"},"action":"apply"})).unwrap();
    let after = inspect_host_specimen(&mut h.app).unwrap();
    let applies = |value: &serde_json::Value| {
        value["runtime"]["probe"]["controllers"].as_array().unwrap().iter().find(|state| state["instance"] == "specimen-0").unwrap()["applies"]
            .as_u64()
            .unwrap()
    };
    assert_eq!(applies(&after), applies(&before) + 1);
    assert_eq!(after["runtime"]["probe"]["content_command_calls"], before["runtime"]["probe"]["content_command_calls"].as_u64().unwrap() + 1);
    assert_eq!(before["session"], after["session"]);
    let rejected = h.app.run_panel_command("panel.action", &serde_json::json!({"target":{"type":"instance","id":"specimen-1"},"action":"apply"}));
    assert_eq!(rejected.unwrap_err().rule, solidcraft_engine::panels::Rule::Capability);
    let undeclared = h.app.run_panel_command("panel.action", &serde_json::json!({"target":{"type":"instance","id":"specimen-0"},"action":"cancel"}));
    assert_eq!(undeclared.unwrap_err().rule, solidcraft_engine::panels::Rule::Capability, "a content action is not automatically a header action");
    let refused = inspect_host_specimen(&mut h.app).unwrap();
    assert_eq!(applies(&after), applies(&refused));
    assert_eq!(after["runtime"]["probe"]["content_command_calls"], refused["runtime"]["probe"]["content_command_calls"]);
    let editor = |value: &serde_json::Value| {
        value["runtime"]["probe"]["controllers"].as_array().unwrap().iter().find(|state| state["instance"] == "specimen-0").unwrap().clone()
    };
    assert_eq!(editor(&after)["cancels"], editor(&refused)["cancels"]);
    assert_eq!(editor(&after)["transaction_live"], editor(&refused)["transaction_live"]);
    assert_eq!(before["session"], refused["session"]);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn desktop_panel_windows_reserve_ids_only_after_commit_and_fit_the_current_monitor() {
    use super::native::{DesktopNativeAdapter, NativeMonitor};

    let mut adapter = DesktopNativeAdapter::default();
    let host = HostId::new("desktop-test").unwrap();
    let section = SectionId::new("desktop-section").unwrap();
    let instance = InstanceId::new("desktop-panel").unwrap();
    let create = [HostEffect::CreateNative { host: host.clone(), section: section.clone() }];
    adapter.prepare(&create).unwrap();
    assert!(adapter.viewport_id(&host).is_none());
    adapter.commit(&create);
    let previous = adapter.viewport_id(&host).unwrap();
    assert!(adapter.owns(&host, &section));
    assert_eq!(adapter.prepare(&create).unwrap_err().rule, Rule::DuplicateId);

    let monitor = egui::Rect::from_min_size(egui::pos2(-1000.0, 0.0), egui::vec2(1000.0, 800.0));
    adapter.update_monitors(|| vec![NativeMonitor { name: Some("Primary".into()), bounds: monitor, scale: 2.0, primary: true }]);
    let ctx = egui::Context::default();
    let min = Size { width: 200.0, height: 100.0 };
    let max = Size { width: 900.0, height: 900.0 };
    let builder = adapter.builder(&ctx, &host, &instance, "Panel".into(), max, (min, max)).unwrap();
    let size = builder.inner_size.unwrap();
    let origin = builder.position.unwrap();
    assert!(monitor.contains_rect(egui::Rect::from_min_size((origin.to_vec2() * 2.0).to_pos2(), size * 2.0)));
    assert_eq!(builder.min_inner_size, Some(egui::vec2(min.width, min.height)));

    adapter.commit(&[HostEffect::ReleaseNative { host: host.clone() }]);
    assert!(adapter.viewport_id(&host).is_none());
    adapter.prepare(&create).unwrap();
    adapter.commit(&create);
    assert_ne!(adapter.viewport_id(&host), Some(previous));
}
