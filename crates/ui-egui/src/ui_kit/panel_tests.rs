use crate::ui_kit::test_support::*;
use crate::ui_kit::*;

use crate::{scenario, theme::Tokens};
use egui::Ui;

#[test]
fn closed_backing_window_noop_does_not_block_real_host_menu_but_visible_returning_window_does() {
    use crate::panel_runtime::{ContentServices, HostControlTarget, HostOptions, HostPointerTarget, PanelMountHarness, WorkspacePanelHost};
    use solidcraft_engine::panels::{
        ActionSink as _, Destination, InstanceId, LogicalRect, Mutation, PanelKind, Point, SectionId, SectionLocation, Size, Zone,
    };
    fn frame(
        ctx: &egui::Context,
        content: &mut PanelMountHarness,
        host: &mut WorkspacePanelHost,
        time: &mut f64,
        events: Vec<egui::Event>,
        position: egui::Pos2,
        open: bool,
    ) -> (bool, bool) {
        content.tick().unwrap();
        host.capture_metrics(&content.runtime, &content.models).unwrap();
        host.capture_reservations(&content.runtime, &content.catalogue).unwrap();
        host.prepare(&content.runtime, LogicalRect { origin: Point { x: 0.0, y: 0.0 }, size: Size { width: 1400.0, height: 1000.0 } }).unwrap();
        *time += 1.0 / 60.0;
        let mut observed = (false, false);
        crate::test_support::frame(
            ctx,
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 1000.0))),
                focused: true,
                time: Some(*time),
                events,
                ..Default::default()
            },
            |ui| {
                Container::begin_host_windows(ui.ctx());
                host.show_deferred(
                    ui,
                    &mut content.runtime,
                    &mut content.native,
                    &mut ContentServices { models: &content.models, commands: &mut content.commands, local: &mut content.local },
                )
                .unwrap();
                let source = host
                    .frame()
                    .unwrap()
                    .controls
                    .iter()
                    .find(|control| {
                        matches!(&control.target,
                HostControlTarget::Pointer(HostPointerTarget::Menu { instance }) if instance.0 == "source")
                    })
                    .unwrap();
                observed.0 = ui.ctx().read_response(source.id).unwrap().clicked();
                let id = egui::Id::unique("returning_backing_feature");
                let container = Container::new(ui.ctx(), Surface::Feature);
                let backing = Panel::feature_frame(ui.ctx(), crate::ui_kit::LayoutMode::Floating, crate::frame::Side::Left);
                let stock = container
                    .window("Returning independent feature")
                    .id(id)
                    .title_bar(false)
                    .collapsible(false)
                    .resizable(false)
                    .scroll(false)
                    .constrain(false)
                    .fade_in(false)
                    .fade_out(false)
                    .fixed_pos(position)
                    .frame(backing);
                let mut open_flag = open;
                let mut body_called = false;
                let windows_before = Container::host_window_regions(ui.ctx());
                // Exercise the real optional-backing allocation path without
                // manufacturing GPU support in this headless test.
                let shown =
                    container::FeatureWindow::new(stock, id, backing).open(&mut open_flag).show_background(ui.ctx(), Some(host_marker), |ui| {
                        body_called = true;
                        ui.set_min_size(egui::vec2(180.0, 90.0));
                        ui.label("Actual foreign feature content");
                        ui.is_visible() && !ui.is_sizing_pass()
                    });
                observed.1 = shown.as_ref().and_then(|shown| shown.inner).unwrap_or(false);
                if !open {
                    assert!(shown.is_none());
                    assert!(!body_called, "closing must skip the body on the first closed frame");
                    assert_eq!(Container::host_window_regions(ui.ctx()), windows_before, "a closed window cannot leave a ghost footprint");
                    let layer = egui::LayerId::new(egui::Order::Middle, id);
                    assert!(
                        ui.ctx().graphics(|graphics| graphics.get(layer).is_some_and(
                            |paint| paint.all_entries().count() > 0 && paint.all_entries().all(|entry| matches!(&entry.shape, egui::Shape::Noop))
                        )),
                        "a closed backing wrapper must really leave only its reserved Noop"
                    );
                } else if observed.1 {
                    assert!(Container::host_window_regions(ui.ctx()).unwrap().contains(&shown.as_ref().unwrap().response.rect));
                }
                host.finish(
                    ui,
                    &mut content.runtime,
                    &mut content.native,
                    &mut ContentServices { models: &content.models, commands: &mut content.commands, local: &mut content.local },
                    &mut content.catalogue,
                )
                .unwrap();
            },
        );
        observed
    }
    fn pointer(at: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
        vec![
            egui::Event::PointerMoved(at),
            egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE },
        ]
    }
    for scale in [1.0, 2.0] {
        let ctx = crate::fonts::test_context();
        ctx.set_pixels_per_point(scale);
        ctx.options_mut(|options| options.max_passes = 1.try_into().unwrap());
        let mut content = PanelMountHarness::new(false).unwrap();
        content
            .dispatch(vec![Mutation::Open {
                kind: PanelKind::new("fixture.log").unwrap(),
                instance: InstanceId::new("source").unwrap(),
                destination: Some(Destination::NewSection {
                    section: SectionId::new("source-section").unwrap(),
                    index: 0,
                    location: SectionLocation::Dock { zone: Zone::Left },
                    size: Size { width: 280.0, height: 240.0 },
                }),
            }])
            .unwrap();
        let before = content.runtime.layout().inspect();
        let commits = content.native.commits;
        let mut host = WorkspacePanelHost::new(HostOptions::default()).unwrap();
        let mut time = 0.0;
        let away = egui::pos2(1600.0, 1200.0);
        for _ in 0..3 {
            frame(&ctx, &mut content, &mut host, &mut time, Vec::new(), away, true);
        }
        let source = host
            .frame()
            .unwrap()
            .controls
            .iter()
            .find(|control| {
                matches!(&control.target,
            HostControlTarget::Pointer(HostPointerTarget::Menu { instance }) if instance.0 == "source")
            })
            .unwrap()
            .clone();
        let at = crate::panel_runtime::egui_rect(source.bounds).center();
        let cover = at - egui::vec2(12.0, 12.0);
        assert!(frame(&ctx, &mut content, &mut host, &mut time, Vec::new(), cover, true).1);
        frame(&ctx, &mut content, &mut host, &mut time, Vec::new(), cover, false);
        let retained = egui::containers::AreaState::load(&ctx, egui::Id::unique("returning_backing_feature")).unwrap();
        assert!(retained.interactable && retained.rect().contains(at), "the closed window must retain its old covering Area state");
        frame(&ctx, &mut content, &mut host, &mut time, pointer(at, true), cover, false);
        assert!(
            frame(&ctx, &mut content, &mut host, &mut time, pointer(at, false), cover, false).0,
            "the earlier actual Menu widget must complete its real click"
        );
        for _ in 0..2 {
            frame(&ctx, &mut content, &mut host, &mut time, Vec::new(), cover, false);
        }
        assert!(host.frame().unwrap().menu_bounds.is_some(), "Noop-only backing must not refuse the real underlying menu intent");
        for pressed in [true, false] {
            frame(
                &ctx,
                &mut content,
                &mut host,
                &mut time,
                vec![egui::Event::Key { key: egui::Key::Escape, physical_key: None, pressed, repeat: false, modifiers: egui::Modifiers::NONE }],
                cover,
                false,
            );
        }
        frame(&ctx, &mut content, &mut host, &mut time, Vec::new(), cover, false);
        frame(&ctx, &mut content, &mut host, &mut time, pointer(at, true), cover, false);
        let returned = frame(&ctx, &mut content, &mut host, &mut time, pointer(at, false), cover, true);
        assert!(returned.0 && returned.1, "the actual Window must return visibly after the earlier Menu click queues its intent");
        for _ in 0..3 {
            frame(&ctx, &mut content, &mut host, &mut time, Vec::new(), cover, false);
        }
        assert!(host.frame().unwrap().menu_bounds.is_none(), "actual visible foreign paint still rejects the source with no latent replay");
        assert_eq!(content.runtime.layout().inspect(), before);
        assert_eq!(content.native.commits, commits);
        assert_eq!(content.commands.calls, 0);
    }
}

#[test]
fn host_window_ledger_is_opt_in_pass_scoped_identity_aware_and_fail_closed() {
    let ctx = crate::fonts::test_context();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
    let first = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(100.0, 80.0));
    let moved = first.translate(egui::vec2(30.0, 40.0));
    let id = egui::Id::unique("actual_window_identity");
    let mut passes = 0;
    crate::test_support::frame(&ctx, egui::RawInput { screen_rect: Some(screen), ..Default::default() }, |ui| {
        passes += 1;
        assert_eq!(Container::host_window_regions(ui.ctx()), Some(Vec::new()));
        Container::record_host_window(ui.ctx(), id, Some(first));
        assert_eq!(Container::host_window_regions(ui.ctx()), Some(Vec::new()), "a producer cannot activate collection without the host");
        Container::begin_host_windows(ui.ctx());
        Container::record_host_window(ui.ctx(), id, Some(first));
        Container::record_host_window(ui.ctx(), id, Some(moved));
        assert_eq!(Container::host_window_regions(ui.ctx()), Some(vec![moved]), "an identity update replaces its old footprint");
        Container::record_host_window(ui.ctx(), id, None);
        assert_eq!(Container::host_window_regions(ui.ctx()), Some(Vec::new()));
        Container::record_host_window(ui.ctx(), id, Some(first));
        if ui.ctx().current_pass_index() == 0 {
            ui.ctx().request_discard("prove persistent window footprints do not leak into a retry pass");
        }
    });
    assert!(passes >= 2);
    crate::test_support::frame(&ctx, egui::RawInput { screen_rect: Some(screen), ..Default::default() }, |ui| {
        assert_eq!(Container::host_window_regions(ui.ctx()), Some(Vec::new()), "a new frame cannot reuse the preceding footprint");
    });
    for bad in [
        egui::Rect::from_min_size(egui::pos2(f32::NAN, 0.0), egui::vec2(10.0, 10.0)),
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(f32::INFINITY, 10.0)),
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::Vec2::ZERO),
    ] {
        crate::test_support::frame(&ctx, egui::RawInput { screen_rect: Some(screen), ..Default::default() }, |ui| {
            Container::begin_host_windows(ui.ctx());
            Container::record_host_window(ui.ctx(), id, Some(bad));
            assert!(Container::host_window_regions(ui.ctx()).is_none());
            Container::record_host_window(ui.ctx(), id, None);
            assert!(Container::host_window_regions(ui.ctx()).is_none(), "removal cannot erase a hostile-footprint failure in this pass");
        });
    }
    crate::test_support::frame(&ctx, egui::RawInput { screen_rect: Some(screen), ..Default::default() }, |ui| {
        Container::begin_host_windows(ui.ctx());
        for index in 0..256 {
            Container::record_host_window(ui.ctx(), egui::Id::unique(("bounded_window", index)), Some(first));
        }
        assert_eq!(Container::host_window_regions(ui.ctx()).unwrap().len(), 256);
        Container::record_host_window(ui.ctx(), egui::Id::unique(("bounded_window", 0)), Some(moved));
        assert_eq!(Container::host_window_regions(ui.ctx()).unwrap().len(), 256, "repainting a known window consumes no extra capacity");
        Container::record_host_window(ui.ctx(), egui::Id::unique(("bounded_window", 256)), Some(first));
        assert!(Container::host_window_regions(ui.ctx()).is_none(), "overflow must refuse the whole snapshot instead of truncating windows");
        Container::record_host_window(ui.ctx(), egui::Id::unique(("bounded_window", 0)), None);
        assert!(Container::host_window_regions(ui.ctx()).is_none());
    });
    let peer = egui::ViewportId::from_hash_of("independent_native_host");
    let mut input = egui::RawInput { viewport_id: peer, screen_rect: Some(screen), ..Default::default() };
    input.viewports.entry(peer).or_default().parent = Some(egui::ViewportId::ROOT);
    crate::test_support::frame(&ctx, input, |ui| {
        assert_eq!(ui.ctx().viewport_id(), peer);
        assert_eq!(Container::host_window_regions(ui.ctx()), Some(Vec::new()), "another viewport cannot inherit root's exhausted ledger");
        Container::begin_host_windows(ui.ctx());
        Container::record_host_window(ui.ctx(), id, Some(moved));
        assert_eq!(Container::host_window_regions(ui.ctx()), Some(vec![moved]));
    });
    crate::test_support::frame(&ctx, egui::RawInput { screen_rect: Some(screen), ..Default::default() }, |ui| {
        Container::begin_host_windows(ui.ctx());
        assert_eq!(Container::host_window_regions(ui.ctx()), Some(Vec::new()), "root's next pass recovers without peer footprints");
    });
}

#[test]
fn actual_feature_and_system_windows_publish_visible_paint_but_popups_and_modals_do_not() {
    for scale in [1.0, 2.0] {
        for background in [false, true] {
            let ctx = crate::fonts::test_context();
            ctx.set_pixels_per_point(scale);
            ctx.options_mut(|options| options.max_passes = 1.try_into().unwrap());
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 800.0));
            for visible in [false, true, true] {
                crate::test_support::frame(&ctx, egui::RawInput { screen_rect: Some(screen), ..Default::default() }, |ui| {
                    Container::begin_host_windows(ui.ctx());
                    let id = egui::Id::unique("actual_feature_ledger_producer");
                    let container = Container::new(ui.ctx(), Surface::Feature);
                    let frame = Panel::feature_frame(ui.ctx(), crate::ui_kit::LayoutMode::Floating, crate::frame::Side::Left);
                    let stock = container
                        .window("Persistent feature")
                        .id(id)
                        .title_bar(false)
                        .collapsible(false)
                        .resizable(false)
                        .scroll(false)
                        .fade_in(false)
                        .fade_out(false)
                        .frame(frame)
                        .fixed_pos(egui::pos2(20.0, 20.0));
                    let shown = container::FeatureWindow::new(stock, id, frame)
                        .show_background(ui.ctx(), background.then_some(host_marker), |ui| {
                            let visible = ui.is_visible() && !ui.is_sizing_pass();
                            ui.disable();
                            ui.set_min_size(egui::vec2(160.0, 90.0));
                            let leaf = ui.button("Disabled content still paints");
                            assert!(!leaf.enabled());
                            visible
                        })
                        .unwrap();
                    assert_eq!(shown.inner, Some(visible));
                    let feature = Container::host_window_regions(ui.ctx()).unwrap();
                    assert_eq!(feature, if visible { vec![shown.response.rect] } else { Vec::new() });
                    Window::new(egui::Id::unique("actual_system_ledger_producer"), "", "Persistent system window")
                        .placed(egui::Align2::LEFT_TOP, egui::pos2(400.0, 20.0))
                        .surface(ui.ctx(), false, |ui| {
                            ui.disable();
                            ui.label("Visible system content");
                            (false, Action::None)
                        });
                    let windows = Container::host_window_regions(ui.ctx()).unwrap();
                    assert_eq!(windows.len(), if visible { 2 } else { 0 });
                    if visible {
                        assert!(windows.contains(&shown.response.rect));
                        assert!(windows.contains(&scenario::rect_of("dialog:window").unwrap()));
                    }
                    Container::new(ui.ctx(), Surface::Popup)
                        .area(egui::Id::unique("transient_popup_not_a_fit_obstacle"))
                        .fixed_pos(egui::pos2(600.0, 400.0))
                        .show(ui.ctx(), |ui| {
                            ui.label("Transient popup");
                        });
                    Container::new(ui.ctx(), Surface::Canvas)
                        .area(egui::Id::unique("model_following_area_not_a_fit_obstacle"))
                        .fixed_pos(egui::pos2(600.0, 500.0))
                        .show(ui.ctx(), |ui| {
                            ui.label("Model-following value box");
                        });
                    Window::new(egui::Id::unique("modal_system_wrapper_not_a_fit_obstacle"), "", "Modal system window").surface(
                        ui.ctx(),
                        true,
                        |ui| {
                            ui.label("Modal system content");
                            (false, Action::None)
                        },
                    );
                    Container::new(ui.ctx(), Surface::Window).modal(egui::Id::unique("modal_not_a_persistent_fit_obstacle")).show(ui.ctx(), |ui| {
                        ui.label("Modal content");
                    });
                    assert_eq!(
                        Container::host_window_regions(ui.ctx()),
                        Some(windows),
                        "popup, modal and model-following wrappers cannot register persistent windows"
                    );
                });
            }
        }
    }
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
                    assert!(matches!(node.role(), egui::accesskit::Role::Button | egui::accesskit::Role::SpinButton | egui::accesskit::Role::Slider));
                    if matches!(&control.target, HostControlTarget::Pointer(HostPointerTarget::Menu { .. })) {
                        assert_eq!(control.accessible_name, crate::i18n::tr("ui.panel_host.menu"));
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
fn universal_kit_chrome_uses_one_container_and_preserves_parent_clipping_and_focus_semantics() {
    use crate::ui_kit::panel_host::{HostChrome, panel_host_control, panel_host_sash, panel_host_surface};
    let mut t = Tokens::get();
    let bevel = crate::theme::Bevel { width: 1.0, highlight: t.form_ink, light: t.control_border, shadow: t.form_dim, dark_shadow: t.border };
    t.borders.primary = Some(egui::Stroke::new(1.0, t.border));
    t.borders.bevel = Some(bevel);
    t.borders.primary_inset = Some(crate::theme::InsetBorder { stroke: egui::Stroke::new(0.75, t.form_error), gap: 3.0 });
    let _tokens = crate::theme::scoped_tokens(t);
    let ctx = crate::fonts::test_context();
    ctx.enable_accesskit();
    let bounds = egui::Rect::from_min_size(egui::pos2(20.0, 30.0), egui::vec2(160.0, 120.0));
    let clip = egui::Rect::from_min_max(bounds.min + egui::vec2(5.0, 5.0), bounds.max);
    let header = egui::Rect::from_min_size(bounds.min, egui::vec2(bounds.width(), 32.0));
    let sash = egui::Rect::from_min_size(egui::pos2(bounds.left(), bounds.bottom() - 4.0), egui::vec2(bounds.width(), 4.0));
    let mut calls = 0;
    let output = crate::test_support::frame(&ctx, egui::RawInput::default(), |ui| {
        ui.set_clip_rect(clip);
        panel_host_surface(
            ui,
            egui::Id::unique("synthetic-section"),
            bounds,
            false,
            panel_host_frame(ui.ctx(), false).fill,
            None,
            |ui, content_clip| {
                calls += 1;
                assert_eq!(ui.clip_rect(), clip);
                assert!(clip.contains_rect(content_clip));
                let outer_frames = ui.stack().iter().filter(|node| node.frame().fill != egui::Color32::TRANSPARENT).count();
                assert!(outer_frames <= 1, "a section must not introduce nested outer frames");
                let title = panel_host_control(
                    ui,
                    egui::Id::unique("synthetic-header"),
                    header,
                    HostChrome {
                        label: "Synthetic long localized panel title",
                        icon: Some("list"),
                        icon_only: false,
                        attention: true,
                        selected: false,
                        enabled: true,
                        drag: true,
                        title: true,
                        grip_only: false,
                        rtl: false,
                    },
                );
                let resize = panel_host_sash(ui, egui::Id::unique("synthetic-sash"), sash, "Resize synthetic neighbours");
                assert!(title.sense.is_focusable());
                assert!(resize.sense.is_focusable());
                assert_eq!(title.rect, header);
                assert_eq!(resize.rect, sash);
            },
        );
    });
    assert!(calls >= 1);
    let tree = output.platform_output.accesskit_update.as_ref().unwrap();
    for label in ["Synthetic long localized panel title", "Resize synthetic neighbours"] {
        assert!(tree.nodes.iter().any(|(_, node)| node.label() == Some(label)), "actual focusable widget: {label}");
    }
    let (mut bevel_edges, mut inset_borders) = (0, 0);
    for shape in &output.shapes {
        assert!(clip.contains_rect(shape.clip_rect), "chrome paint must not escape its inherited clip");
        crate::test_support::each_shape(&shape.shape, &mut |paint| match paint {
            egui::Shape::LineSegment { stroke, .. }
                if stroke.width == bevel.width && [bevel.highlight, bevel.light, bevel.shadow, bevel.dark_shadow].contains(&stroke.color) =>
            {
                bevel_edges += 1;
            }
            egui::Shape::Rect(rect) if rect.stroke.color == t.form_error => {
                assert!(!clip.contains_rect(rect.rect), "the partial clip must actually trim the inset border");
                inset_borders += 1;
            }
            _ => {}
        });
    }
    assert!(bevel_edges > 0 && inset_borders > 0, "exercise both decorations under the inherited partial clip");
}

#[test]
fn universal_surface_uses_adaptive_workspace_fill_and_rounding_with_safe_content_clip() {
    use crate::ui_kit::LayoutMode;
    let bounds = egui::Rect::from_min_size(egui::pos2(40.0, 40.0), egui::vec2(320.0, 180.0));
    for dark in [false, true] {
        let mut tokens = if dark { Tokens::dark() } else { Tokens::light() };
        tokens.panel = tokens.panel.gamma_multiply(0.8);
        let _tokens = crate::theme::scoped_tokens(tokens);
        for layout in [LayoutMode::Floating, LayoutMode::Docked] {
            for free_card in [false, true] {
                let ctx = crate::fonts::test_context();
                layout.publish(&ctx);
                let shared = if free_card { Container::floating_card(Surface::Panel) } else { Container::new(&ctx, Surface::Panel) };
                let expected = shared.frame(&ctx, tokens.panel, crate::frame::Side::Left);
                if free_card {
                    // A new floating Area first measures invisibly, even with fade disabled.
                    crate::test_support::frame(&ctx, egui::RawInput { time: Some(0.0), ..Default::default() }, |ui| {
                        panel_host_surface(ui, egui::Id::unique("adaptive-surface"), bounds, free_card, expected.fill, None, |_, _| {});
                    });
                }
                for time in [1.0, 2.0] {
                    let output = crate::test_support::frame(&ctx, egui::RawInput { time: Some(time), ..Default::default() }, |ui| {
                        panel_host_surface(ui, egui::Id::unique("adaptive-surface"), bounds, free_card, expected.fill, None, |ui, clip| {
                            assert!(ui.is_visible() && !ui.is_sizing_pass(), "assert the first visible paint and its subsequent frame");
                            assert!(bounds.contains_rect(clip));
                            let stroke = expected.stroke.width;
                            for (corner, center, radius) in [
                                (clip.left_top(), bounds.left_top(), expected.corner_radius.nw),
                                (clip.right_top(), bounds.right_top(), expected.corner_radius.ne),
                                (clip.left_bottom(), bounds.left_bottom(), expected.corner_radius.sw),
                                (clip.right_bottom(), bounds.right_bottom(), expected.corner_radius.se),
                            ] {
                                let radius = f32::from(radius);
                                let inward = egui::vec2(
                                    if center.x == bounds.left() { radius } else { -radius },
                                    if center.y == bounds.top() { radius } else { -radius },
                                );
                                if radius > stroke {
                                    assert!(corner.distance(center + inward) <= radius - stroke + 0.001);
                                }
                            }
                            // A controller may paint an oversized rectangular body;
                            // its inherited content clip must still protect corners.
                            ui.painter().with_clip_rect(clip).rect_filled(bounds, 0, tokens.control_fill);
                        });
                    });
                    let mut backgrounds = 0;
                    for shape in &output.shapes {
                        crate::test_support::each_shape(&shape.shape, &mut |shape| {
                            if let egui::Shape::Rect(rect) = shape
                                && rect.rect == bounds
                                && rect.stroke == expected.stroke
                            {
                                assert_eq!(rect.fill, expected.fill, "shared frame alpha is stable from the first visible paint");
                                assert_eq!(rect.corner_radius, expected.corner_radius);
                                assert!(rect.fill.a() < 255, "the host must retain the shared frame's alpha");
                                backgrounds += 1;
                            }
                        });
                    }
                    assert!(backgrounds > 0, "actual bounded background must use the shared frame");
                }
            }
        }
    }
}

#[test]
fn universal_header_reuses_title_font_and_six_dot_grip_without_idle_control_tiles() {
    let t = Tokens::get();
    let title = egui::Rect::from_min_size(egui::pos2(30.0, 30.0), egui::vec2(300.0, t.form_header_height));
    let tab = title.translate(egui::vec2(0.0, title.height()));
    let group = tab.translate(egui::vec2(0.0, tab.height()));
    for rtl in [false, true] {
        let ctx = crate::fonts::test_context();
        let output = crate::test_support::frame(&ctx, egui::RawInput::default(), |ui| {
            for (bounds, is_title, label) in [(title, true, "Shared title"), (tab, false, "Shared tab")] {
                let response = panel_host_control(
                    ui,
                    egui::Id::unique(label),
                    bounds,
                    HostChrome {
                        label,
                        icon: None,
                        icon_only: false,
                        attention: false,
                        selected: false,
                        enabled: true,
                        drag: true,
                        title: is_title,
                        grip_only: false,
                        rtl,
                    },
                );
                assert_eq!(response.rect, bounds, "paint insets must preserve the exact target");
                assert!(response.sense.is_focusable());
            }
            let grip = egui::Rect::from_center_size(
                egui::pos2(if rtl { group.right() - t.space_10 } else { group.left() + t.space_10 }, group.center().y),
                egui::vec2(t.space_10 + t.space_8, group.height()),
            );
            let response = panel_host_control(
                ui,
                egui::Id::unique("group-grip"),
                grip,
                HostChrome {
                    label: "Accessible active panel",
                    icon: None,
                    icon_only: false,
                    attention: false,
                    selected: false,
                    enabled: true,
                    drag: true,
                    title: true,
                    grip_only: true,
                    rtl,
                },
            );
            assert_eq!(response.rect, grip);
            assert!(response.sense.is_focusable());
        });
        let (mut titles, mut tabs, mut dots, mut group_dots) = (0, 0, 0, 0);
        for clipped in &output.shapes {
            crate::test_support::each_shape(&clipped.shape, &mut |shape| match shape {
                egui::Shape::Text(text) if text.galley.job.text == "Shared title" => {
                    assert_eq!(text.galley.job.sections.first().unwrap().format.font_id, crate::fonts::semibold(t.font_heading));
                    titles += 1;
                }
                egui::Shape::Text(text) if text.galley.job.text == "Shared tab" => {
                    assert_eq!(text.galley.job.sections.first().unwrap().format.font_id, egui::FontId::proportional(t.font_label));
                    tabs += 1;
                }
                egui::Shape::Text(text) => {
                    assert_ne!(text.galley.job.text, "Accessible active panel", "grip accessibility name is not a duplicate painted title")
                }
                egui::Shape::Circle(dot) if title.contains(dot.center) && dot.fill == t.text_dim => {
                    assert_eq!(dot.center.x > title.center().x, rtl, "grip follows the leading edge");
                    dots += 1;
                }
                egui::Shape::Circle(dot) if group.contains(dot.center) && dot.fill == t.text_dim => {
                    group_dots += 1;
                }
                egui::Shape::Rect(rect) => assert_ne!(rect.fill, t.panel_header, "idle controls share the outer background"),
                _ => {}
            });
        }
        assert_eq!((titles, tabs, dots, group_dots), (1, 1, 6, 6));
    }
}

#[test]
fn universal_drag_source_changes_only_label_ink_and_keeps_existing_fonts() {
    let t = Tokens::get();
    for (title, selected, attention, enabled) in
        [(true, false, false, true), (false, true, true, true), (false, false, false, true), (false, false, false, false)]
    {
        let ctx = crate::fonts::test_context();
        let bounds = egui::Rect::from_min_size(egui::pos2(30.0, 30.0), egui::vec2(300.0, t.form_header_height));
        let other = bounds.translate(egui::vec2(0.0, bounds.height()));
        let ink = if !enabled {
            t.form_dim
        } else if attention {
            t.form_primary
        } else {
            t.form_ink
        };
        let font = if title {
            crate::fonts::semibold(t.font_heading)
        } else if selected {
            crate::fonts::semibold(t.font_label)
        } else {
            egui::FontId::proportional(t.font_label)
        };
        let output = crate::test_support::frame(&ctx, egui::RawInput::default(), |ui| {
            for (rect, label, opacity) in [(bounds, "Moved title", 0.5), (other, "Other title", 1.0)] {
                let response = panel_host_control_label_opacity(
                    ui,
                    egui::Id::unique(label),
                    rect,
                    HostChrome { label, icon: None, icon_only: false, attention, selected, enabled, drag: true, title, grip_only: false, rtl: false },
                    opacity,
                );
                assert_eq!(response.rect, rect);
                assert_eq!(response.sense, if enabled { egui::Sense::click_and_drag() } else { egui::Sense::hover() });
            }
        });
        let (mut moved, mut others, mut dots) = (0, 0, 0);
        for clipped in &output.shapes {
            crate::test_support::each_shape(&clipped.shape, &mut |shape| match shape {
                egui::Shape::Text(text) if matches!(text.galley.job.text.as_str(), "Moved title" | "Other title") => {
                    let dim = text.galley.job.text == "Moved title";
                    for section in &text.galley.job.sections {
                        assert_eq!(section.format.color, ink.gamma_multiply(if dim { 0.5 } else { 1.0 }));
                        assert_eq!(section.format.font_id, font);
                    }
                    if dim {
                        moved += 1;
                    } else {
                        others += 1;
                    }
                }
                egui::Shape::Circle(dot) if title && (bounds.contains(dot.center) || other.contains(dot.center)) => {
                    assert_eq!(dot.fill, t.text_dim, "grip ink remains unchanged");
                    dots += 1;
                }
                egui::Shape::Rect(rect) if selected && (rect.rect.intersects(bounds) || rect.rect.intersects(other)) => {
                    assert_eq!(rect.fill, t.selected_bg, "selection fill remains unchanged");
                }
                _ => {}
            });
        }
        assert_eq!((moved, others, dots), (1, 1, if title { 12 } else { 0 }));
    }
}

#[test]
fn universal_drag_badge_keeps_display_order_on_one_line_and_clamps_without_allocating_input_or_layout() {
    let t = Tokens::get();
    let domain = egui::Rect::from_min_max(egui::pos2(20.0, 30.0), egui::pos2(440.0, 330.0));
    let logical = "אחד שניים שלושה ארבעה חמישה שישה שבעה שמונה תשעה עשרה אחרון";
    let hebrew = crate::bidi::visual(logical).into_owned();
    assert_ne!(hebrew, logical, "exercise an already display-ordered eleven-word Hebrew title");
    let narrow = egui::Rect::from_min_size(domain.min, egui::vec2(180.0, 600.0));
    for (scale, rtl) in [(1.0, false), (2.0, true)] {
        let ctx = crate::fonts::test_context();
        ctx.set_pixels_per_point(scale);
        for (title, domain) in
            [("Full title".to_owned(), domain), ("Full current presentation title without ellipsis ".repeat(5), domain), (hebrew.clone(), narrow)]
        {
            let clip = domain.shrink(20.0);
            for at in [
                clip.center(),
                clip.left_top() + egui::vec2(1.0, 1.0),
                clip.right_bottom() - egui::vec2(1.0, 1.0),
                egui::pos2(-20.0, -20.0),
                egui::pos2(f32::NAN, 100.0),
            ] {
                let visible = at.is_finite() && clip.contains(at);
                let mut layer = None;
                let output = crate::test_support::frame(&ctx, egui::RawInput::default(), |ui| {
                    ui.set_clip_rect(clip);
                    layer = Some(egui::LayerId::new(egui::Order::Tooltip, ui.scope_id().with("panel_preview")));
                    let before = ui.min_rect();
                    panel_host_drag_title(ui, &title, at, domain, rtl);
                    assert_eq!(ui.min_rect(), before, "paint-only feedback allocates no layout");
                });
                let (mut labels, mut backgrounds) = (0, 0);
                for clipped in &output.shapes {
                    crate::test_support::each_shape(&clipped.shape, &mut |shape| match shape {
                        egui::Shape::Text(text) if text.galley.job.text == title => {
                            assert!(clip.contains_rect(clipped.clip_rect));
                            let text_bounds = egui::Rect::from_min_size(text.pos, text.galley.size());
                            assert!(text_bounds.intersects(clipped.clip_rect));
                            let padding = t.space_8.min(clip.width() / 4.0);
                            let aligned = if rtl {
                                text_bounds.right() - (clipped.clip_rect.right() - padding)
                            } else {
                                text_bounds.left() - (clipped.clip_rect.left() + padding)
                            };
                            assert!(aligned.abs() < 0.01, "keep the reading start visible when the full galley exceeds its clip");
                            assert!(
                                text.galley.job.sections.iter().all(|section| section.format.color == t.form_ink
                                    && section.format.font_id == egui::FontId::proportional(t.font_label))
                            );
                            assert_eq!(text.galley.rows.len(), 1, "never reflow already display-ordered words");
                            if title == hebrew {
                                assert_eq!(text.galley.job.text, hebrew, "preserve display order without a second bidi pass");
                                assert!(text_bounds.width() > clipped.clip_rect.width(), "the narrow Hebrew case must exercise clipping");
                                if rtl {
                                    assert!(text_bounds.left() < clipped.clip_rect.left());
                                }
                            }
                            labels += 1;
                        }
                        egui::Shape::Rect(rect) if rect.fill == t.dialog_body.to_opaque() && rect.corner_radius == t.radius_sm.into() => {
                            assert!(clip.contains_rect(rect.rect));
                            assert_ne!(ctx.layer_id_at(rect.rect.center()), layer, "no tooltip Area or input shield");
                            if title == "Full title" && at == clip.center() {
                                assert_eq!(rect.rect.center().x < at.x, rtl, "cursor offset mirrors with direction");
                            }
                            backgrounds += 1;
                        }
                        _ => {}
                    });
                }
                assert_eq!((labels, backgrounds), if visible { (1, 1) } else { (0, 0) });
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
    assert!(section.outer.size.height <= crate::theme::PANEL_INLINE_MAX_HEIGHT);
    assert!(section.body.is_none() && section.footer.is_none());
    assert!(!frame.controls.iter().any(|control| matches!(control.target, HostControlTarget::Pointer(HostPointerTarget::HeaderAction { .. }))));
    let runtime = h.app.panel_runtime().unwrap();
    let instance = runtime.layout().layout().instance(&section.instance).unwrap();
    assert!(runtime.layout().registry().get(&instance.kind).unwrap().header_actions.is_empty(), "the inline log declares no custom or debug actions");
    assert!(frame.controls.iter().any(|control| matches!(control.target, HostControlTarget::Pointer(HostPointerTarget::Menu { .. }))));
    assert!(frame.controls.iter().any(|control| matches!(control.target, HostControlTarget::Pointer(HostPointerTarget::Collapse { .. }))));
    assert!(h.widget(crate::i18n::tr("ui.panel_fixture.inline")).is_some());
    assert!(h.widget(crate::i18n::tr("ui.panel_fixture.caption")).is_none());
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

#[derive(Clone, Copy, Debug)]
enum BlurHost {
    Frame,
    Edge(Edge),
    Window,
}

const BLUR_HOSTS: [BlurHost; 6] = [
    BlurHost::Frame,
    BlurHost::Edge(Edge::Left),
    BlurHost::Edge(Edge::Right),
    BlurHost::Edge(Edge::Top),
    BlurHost::Edge(Edge::Bottom),
    BlurHost::Window,
];

struct HostMarker {
    frame: egui::Frame,
    opacity: f32,
}

fn host_marker(painter: &egui::Painter, frame: egui::Frame, content: egui::Rect, ordinary: egui::Shape) -> egui::Shape {
    egui::Shape::Vec(vec![
        egui::Shape::Callback(egui::PaintCallback {
            rect: frame.widget_rect(content),
            callback: std::sync::Arc::new(HostMarker { frame, opacity: painter.opacity() }),
        }),
        ordinary,
    ])
}

/// Compare the actual stock hosts against the app helpers without requiring a GPU.
/// mode: 0 = stock, 1 = production zero path, 2 = injected positive marker.
fn blur_host_fixture(host: BlurHost, mode: u8, position: egui::Pos2, transform: egui::emath::TSTransform) -> (egui::FullOutput, String) {
    let ctx = crate::fonts::test_context();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(640.0, 480.0));
    let container = Container::for_layout(crate::ui_kit::LayoutMode::Floating, Surface::Feature);
    let frame = container
        .frame(&ctx, egui::Color32::from_rgba_unmultiplied(40, 80, 100, 180), crate::frame::Side::Left)
        .stroke(egui::Stroke::new(2.5, egui::Color32::WHITE))
        .inner_margin(egui::Margin { left: 9, right: 13, top: 7, bottom: 11 })
        .outer_margin(egui::Margin { left: -2, right: 3, top: -1, bottom: 4 })
        .corner_radius(egui::CornerRadius { nw: 3, ne: 7, sw: 11, se: 15 });
    let id = egui::Id::unique("stock host fixture");
    let mut output = egui::FullOutput::default();
    let mut observation = String::new();
    for _ in 0..3 {
        output = crate::test_support::frame(&ctx, egui::RawInput { screen_rect: Some(screen), ..Default::default() }, |ui| {
            ui.multiply_opacity(0.75);
            let mut body = |ui: &mut egui::Ui| {
                let stack: Vec<_> = ui.stack().iter().map(|node| (node.scope_id, node.kind(), *node.frame())).collect();
                let semantic = format!("{:?}", (ui.scope_id(), ui.max_rect(), ui.clip_rect(), ui.stack().bg_color(), stack));
                ui.allocate_space(egui::vec2(83.0, 47.0));
                let response = ui.button("Content stays above its frame");
                // A child changing its content clip must not cut the host's padding/border.
                ui.set_clip_rect(ui.clip_rect().shrink(6.0));
                observation = format!("{semantic} {:?}", (response.id, response.rect, ui.min_rect()));
            };
            let background: Option<container::Background> = (mode == 2).then_some(host_marker);
            let response = match host {
                BlurHost::Frame => {
                    if mode == 0 {
                        let shown = frame.show(ui, &mut body);
                        Container::paint_inset(ui.painter(), shown.response.rect - frame.outer_margin, frame);
                        shown.response
                    } else if mode == 1 {
                        container.show_frame(ui, frame, screen, &mut body).response
                    } else {
                        container.show_frame_background(ui, frame, screen, background, &mut body).response
                    }
                }
                BlurHost::Edge(edge) => {
                    let panel = container.edge(id, edge).exact_size(170.0).resizable(false);
                    if mode == 0 {
                        panel.frame(frame).show(ui, &mut body).response
                    } else if mode == 1 {
                        container.show_edge(ui, panel, frame, &mut body).response
                    } else {
                        Container::show_edge_background(ui, panel, frame, background, &mut body).response
                    }
                }
                BlurHost::Window => {
                    let window = container
                        .window("Stock host fixture")
                        .id(id)
                        .title_bar(false)
                        .collapsible(false)
                        .resizable(false)
                        .scroll(false)
                        .fade_in(false)
                        .fade_out(false)
                        .constrain(false)
                        .fixed_pos(position)
                        .frame(frame);
                    if mode == 0 {
                        window.show(&ctx, &mut body).unwrap().response
                    } else {
                        let window = container::FeatureWindow::new(window, id, frame);
                        if mode == 1 { window.show(&ctx, &mut body) } else { window.show_background(&ctx, background, &mut body) }.unwrap().response
                    }
                }
            };
            observation.push_str(&format!(" {:?}", (response.id, response.rect)));
            let layer = if matches!(host, BlurHost::Window) { egui::LayerId::new(egui::Order::Middle, id) } else { ui.layer_id() };
            ctx.set_transform_layer(layer, transform);
        });
    }
    (output, observation)
}

fn blur_host_shapes(output: &egui::FullOutput, omit_markers: bool) -> Vec<String> {
    let mut shapes = Vec::new();
    for clipped in &output.shapes {
        crate::test_support::each_shape(&clipped.shape, &mut |shape| {
            if omit_markers && (matches!(shape, egui::Shape::Noop) || matches!(shape, egui::Shape::Callback(cb) if cb.callback.is::<HostMarker>())) {
                return;
            }
            shapes.push(format!("{:?} {shape:?}", clipped.clip_rect));
        });
    }
    shapes
}

#[test]
fn blur_stock_hosts_zero_has_identical_shapes_noops_geometry_and_ids() {
    for host in BLUR_HOSTS {
        let (stock, semantics) = blur_host_fixture(host, 0, egui::pos2(30.0, 35.0), egui::emath::TSTransform::IDENTITY);
        let (off, off_semantics) = blur_host_fixture(host, 1, egui::pos2(30.0, 35.0), egui::emath::TSTransform::IDENTITY);
        assert_eq!(semantics, off_semantics, "{host:?}");
        assert_eq!(blur_host_shapes(&stock, false), blur_host_shapes(&off, false), "no extra shapes, including Noop slots: {host:?}");
    }
}

#[test]
fn blur_stock_window_open_flag_preserves_closed_close_and_reopen_behavior() {
    fn record(mode: u8, omit_markers: bool) -> Vec<(Vec<String>, String)> {
        let ctx = crate::fonts::test_context();
        let container = Container::for_layout(crate::ui_kit::LayoutMode::Floating, Surface::Feature);
        let frame = container.frame(&ctx, Tokens::get().dialog_body, crate::frame::Side::Left);
        let id = egui::Id::unique("open flag fixture");
        let mut recorded = Vec::new();
        for (index, mut keep) in [false, false, true, true, true, false, false, false, true, true, true].into_iter().enumerate() {
            let initially_open = keep;
            let mut observation = String::new();
            let mut called = false;
            let output = crate::test_support::frame(&ctx, egui::RawInput { time: Some(index as f64), ..Default::default() }, |_| {
                Container::begin_host_windows(&ctx);
                let window = container
                    .window("Open flag fixture")
                    .id(id)
                    .title_bar(false)
                    .collapsible(false)
                    .resizable(false)
                    .scroll(false)
                    .fade_in(false)
                    .fade_out(false)
                    .frame(frame);
                let mut body = |ui: &mut egui::Ui| {
                    called = true;
                    ui.label("Window body");
                    if index == 4 {
                        // Window contains a closable Collapsible even with title/collapse
                        // controls disabled. Target the Window, not that nearer ancestor.
                        ui.close_kind(egui::UiKind::Window);
                    }
                };
                let shown = if mode == 0 {
                    // Compare open windows with stock geometry and close handling;
                    // explicitly closed features skip stock's animated showing.
                    if initially_open { window.open(&mut keep).show(&ctx, &mut body) } else { None }
                } else {
                    // A long-lived builder must accept this frame-local mutable borrow.
                    let window: container::FeatureWindow<'static> = container::FeatureWindow::new(window, id, frame);
                    let window = window.open(&mut keep);
                    if mode == 1 { window.show(&ctx, &mut body) } else { window.show_background(&ctx, Some(host_marker), &mut body) }
                };
                if !initially_open {
                    assert!(shown.is_none(), "initially closed and newly closed windows stay hidden immediately");
                    assert_eq!(Container::host_window_regions(&ctx), Some(Vec::new()), "closed windows publish no geometry");
                }
                if index == 4 {
                    assert!(shown.as_ref().is_some_and(|shown| shown.response.should_close()), "the Window response carries the close request");
                }
                observation = format!("{:?}", (keep, shown.map(|shown| (shown.response.id, shown.response.rect, shown.inner))));
            });
            if !initially_open {
                assert!(!called);
            }
            if index == 4 {
                assert!(!keep, "stock close requests update the caller's flag");
            }
            if index == 10 {
                assert!(called && keep, "the same persistent window can reopen");
            }
            let mut markers = 0;
            for clipped in &output.shapes {
                crate::test_support::each_shape(&clipped.shape, &mut |shape| {
                    if matches!(shape, egui::Shape::Callback(cb) if cb.callback.is::<HostMarker>()) {
                        markers += 1;
                    }
                });
            }
            if !called {
                assert_eq!(markers, 0, "hidden windows emit no marker");
            }
            if mode == 2 && index == 10 {
                assert_eq!(markers, 1, "reopened windows regain their marker");
            }
            recorded.push((blur_host_shapes(&output, omit_markers), observation));
        }
        recorded
    }
    assert_eq!(record(0, false), record(1, false), "zero keeps open stock shapes and Noop counts, with immediate closed visibility");
    assert_eq!(record(0, true), record(2, true), "positive markers preserve close handling, geometry and IDs, with immediate closed visibility");
}

#[test]
fn blur_stock_hosts_marker_precedes_original_chrome_with_semantics_and_clip_intact() {
    for host in BLUR_HOSTS {
        let position = egui::pos2(30.0, 35.0);
        let (stock, semantics) = blur_host_fixture(host, 0, position, egui::emath::TSTransform::IDENTITY);
        let (marked, marked_semantics) = blur_host_fixture(host, 2, position, egui::emath::TSTransform::IDENTITY);
        assert_eq!(semantics, marked_semantics, "bg_color, frame metadata, geometry and persistent IDs: {host:?}");
        assert_eq!(blur_host_shapes(&stock, true), blur_host_shapes(&marked, true), "original shadow/fill/stroke/content: {host:?}");
        let mut marker = None;
        let mut fills = 0;
        let mut callbacks = 0;
        let mut shadows = 0;
        for clipped in &marked.shapes {
            crate::test_support::each_shape(&clipped.shape, &mut |shape| match shape {
                egui::Shape::Callback(cb) if cb.callback.is::<HostMarker>() => {
                    let info = cb.callback.downcast_ref::<HostMarker>().unwrap();
                    assert_eq!(info.frame.stroke.width, 2.5);
                    assert_eq!(info.opacity, if matches!(host, BlurHost::Window) { 1.0 } else { 0.75 });
                    marker = Some((cb.rect, clipped.clip_rect, info.frame.corner_radius));
                    callbacks += 1;
                }
                egui::Shape::Rect(rect) if rect.blur_width > 0.0 => {
                    assert!(marker.is_some(), "the marker must also precede the original shadow: {host:?}");
                    shadows += 1;
                }
                egui::Shape::Rect(rect) if marker.is_some_and(|(r, _, _)| r == rect.rect) => {
                    let (_, clip, corners) = marker.unwrap();
                    assert_eq!(rect.corner_radius, corners);
                    assert_eq!(clip, clipped.clip_rect, "use the outer host clip, including padding/border");
                    assert!(clip.contains(rect.rect.center()));
                    fills += 1;
                }
                _ => {}
            });
        }
        assert_eq!(callbacks, 1, "{host:?}");
        assert_eq!(shadows, 1, "{host:?}");
        assert_eq!(fills, 1, "marker before the matching original fill: {host:?}");
    }
}

#[test]
fn blur_stock_hosts_late_scaled_layers_and_moved_windows_keep_final_geometry() {
    let transform = egui::emath::TSTransform { translation: egui::vec2(19.0, -7.0), scaling: 0.75 };
    for host in BLUR_HOSTS {
        for position in [egui::pos2(30.0, 35.0), egui::pos2(-8.0, 135.0)] {
            let (local, _) = blur_host_fixture(host, 2, position, egui::emath::TSTransform::IDENTITY);
            let (moved, _) = blur_host_fixture(host, 2, position, transform);
            let marker = |output: &egui::FullOutput| {
                let mut result = None;
                for clipped in &output.shapes {
                    crate::test_support::each_shape(&clipped.shape, &mut |shape| {
                        if let egui::Shape::Callback(cb) = shape
                            && cb.callback.is::<HostMarker>()
                        {
                            result = Some((cb.rect, clipped.clip_rect));
                        }
                    });
                }
                result.unwrap()
            };
            let (rect, clip) = marker(&local);
            let (moved_rect, moved_clip) = marker(&moved);
            assert_eq!(moved_rect, transform.mul_rect(rect), "{host:?}");
            assert_eq!(moved_clip, transform.mul_rect(clip), "host clipping transforms once: {host:?}");
        }
    }
}

#[test]
fn blur_stock_frame_marker_uses_final_joined_corner_radii() {
    let ctx = crate::fonts::test_context();
    let output = crate::test_support::frame(&ctx, egui::RawInput::default(), |ui| {
        let host = Container::for_layout(crate::ui_kit::LayoutMode::Docked, Surface::Feature).joined(DockEdges::default());
        let frame = host.frame(ui.ctx(), Tokens::get().dialog_body, crate::frame::Side::Left);
        let bounds = ui.available_rect_before_wrap();
        host.show_frame_background(ui, frame, bounds, Some(host_marker), |ui| {
            ui.allocate_space(egui::vec2(90.0, 40.0));
        });
    });
    let mut matched = false;
    for clipped in &output.shapes {
        crate::test_support::each_shape(&clipped.shape, &mut |shape| {
            if let egui::Shape::Callback(cb) = shape
                && let Some(marker) = cb.callback.downcast_ref::<HostMarker>()
            {
                assert_eq!(marker.frame.corner_radius, egui::CornerRadius { nw: 0, ne: 0, sw: 0, se: Tokens::get().radius_lg });
                matched = true;
            }
        });
    }
    assert!(matched);
}

#[test]
fn blur_zero_has_no_marker_or_tint_override() {
    let mut state = crate::blur::State::default();
    state.set_radius(0.0).unwrap();
    let ctx = crate::fonts::test_context();
    let _scope = state.begin(&ctx, true);
    assert!(!crate::blur::enabled());
    let fill = egui::Color32::from_rgba_unmultiplied(40, 80, 100, 180);
    assert_eq!(crate::blur::panel_fill(fill), fill);
    let original = egui::Shape::rect_filled(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(80.0, 40.0)), 9, fill);
    let painter = ctx.layer_painter(egui::LayerId::background());
    let unchanged = crate::blur::decorate(&painter, egui::Frame::NONE.fill(fill), egui::Rect::NOTHING, original.clone());
    assert_eq!(format!("{original:?}"), format!("{unchanged:?}"));
    assert_eq!(state.counters().markers_emitted, 0);
    assert_eq!(state.counters().textures_created, 0);
}

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
    for layout in [crate::ui_kit::LayoutMode::Docked, crate::ui_kit::LayoutMode::Floating] {
        let ctx = crate::fonts::test_context();
        let vp = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(640.0, 480.0));
        for _ in 0..3 {
            scenario::clear_handles();
            crate::test_support::frame(&ctx, egui::RawInput { screen_rect: Some(vp), ..Default::default() }, |_| {
                let side = crate::frame::Side::Right;
                let frame = Panel::feature_frame(&ctx, layout, side);
                let anchor = egui::pos2(vp.right() - crate::ui_kit::SHELL_GAP, vp.top());
                let panel = Panel::new("Fillet", egui::Id::unique("gallery_feature"), anchor, vp, side, frame);
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

/// One frame of a feature panel with `rows` body rows, the given `placement` published.
fn feature_frame_with(
    ctx: &egui::Context,
    placement: DialogButtons,
    rows: usize,
    valid: bool,
    collapsed: Option<&mut bool>,
    events: Vec<egui::Event>,
) -> (bool, Action) {
    let vp = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(640.0, 480.0));
    scenario::clear_handles();
    let mut result = (false, Action::None);
    let mut collapsed = collapsed;
    crate::test_support::frame(ctx, egui::RawInput { screen_rect: Some(vp), events, focused: true, ..Default::default() }, |_| {
        placement.publish(ctx);
        let side = crate::frame::Side::Right;
        let frame = Panel::feature_frame(ctx, crate::ui_kit::LayoutMode::Floating, side);
        let anchor = egui::pos2(vp.right() - crate::ui_kit::SHELL_GAP, vp.top());
        let panel = Panel::new("Fillet", egui::Id::unique("top_actions"), anchor, vp, side, frame);
        panel.window(ctx).show(ctx, |ui| {
            let heading = FeatureOptions {
                icon: "fillet",
                title: "Fillet",
                drag_key: "top_actions",
                moved: egui::Vec2::ZERO,
                docked: anchor,
                apply_label: Some("Apply"),
            };
            let body = |ui: &mut Ui| {
                for row_index in 0..rows {
                    text(ui, &format!("Input {row_index}"), Text::Note("row"));
                }
                ComputeStatus { text: None, busy: false, applying: false }
            };
            result = match collapsed.as_deref_mut() {
                Some(collapsed) => panel.feature_collapsible(ui, heading, false, valid, collapsed, body),
                None => panel.feature(ui, heading, false, valid, body),
            };
        });
    });
    result
}

#[test]
fn top_placement_puts_the_actions_in_the_header_row_where_they_stay() {
    let ctx = crate::fonts::test_context();
    for _ in 0..3 {
        feature_frame_with(&ctx, DialogButtons::Top, 2, true, None, Vec::new());
    }
    let header = scenario::rect_of("dialog:header").unwrap();
    assert_eq!(header.height(), Tokens::get().form_header_height);
    assert!(scenario::rect_of("dialog:footer").is_none(), "the footer row is gone");
    let (ok, apply, cancel) =
        (scenario::rect_of("dialog:ok").unwrap(), scenario::rect_of("dialog:apply").unwrap(), scenario::rect_of("dialog:cancel").unwrap());
    for (name, rect) in [("ok", ok), ("apply", apply), ("cancel", cancel)] {
        assert!(header.contains_rect(rect), "{name} {rect:?} sits in the header {header:?}");
        assert_eq!(rect.height(), Tokens::get().form_button_height, "{name}");
    }
    assert!(cancel.right() < apply.left() && apply.right() < ok.left(), "the footer's order: Cancel, Apply, then OK at the right");
    assert_eq!(scenario::handle_at("dialog:close"), Some(cancel.center()), "Cancel stands in for the close button");
    assert!(scenario::handle_at("dialog:OK").is_some() && scenario::handle_at("dialog:Cancel").is_some());
    assert!(scenario::handle_at("dialog:title").is_some_and(|at| at.x < cancel.left()), "the title drags clear of the buttons");
    assert_eq!(scenario::count_of("dialog:ok:enabled"), Some(1.0));

    // More inputs, a longer card: the buttons stay where they were.
    for _ in 0..2 {
        feature_frame_with(&ctx, DialogButtons::Top, 24, true, None, Vec::new());
    }
    assert_eq!(scenario::rect_of("dialog:ok"), Some(ok));
    assert_eq!(scenario::rect_of("dialog:cancel"), Some(cancel));

    // OK greys out exactly as it does in the footer, and a click on it then does nothing.
    feature_frame_with(&ctx, DialogButtons::Top, 2, false, None, Vec::new());
    assert_eq!(scenario::count_of("dialog:ok:enabled"), Some(0.0));
    let at = ok.center();
    feature_frame_with(&ctx, DialogButtons::Top, 2, false, None, crate::test_support::pointer(at, true));
    assert_eq!(feature_frame_with(&ctx, DialogButtons::Top, 2, false, None, crate::test_support::pointer(at, false)).1, Action::None);
    feature_frame_with(&ctx, DialogButtons::Bottom, 2, false, None, Vec::new());
    assert_eq!(scenario::count_of("dialog:ok:enabled"), Some(0.0), "the same state at the bottom");

    // The buttons and the keys answer as the footer's do.
    for (target, expected) in [(ok, Action::Ok), (apply, Action::Apply), (cancel, Action::Cancel)] {
        feature_frame_with(&ctx, DialogButtons::Top, 2, true, None, Vec::new());
        feature_frame_with(&ctx, DialogButtons::Top, 2, true, None, crate::test_support::pointer(target.center(), true));
        let clicked = feature_frame_with(&ctx, DialogButtons::Top, 2, true, None, crate::test_support::pointer(target.center(), false));
        assert_eq!(clicked, (false, expected));
    }
    for (key, expected) in [(egui::Key::Escape, Action::Cancel), (egui::Key::Enter, Action::Ok)] {
        feature_frame_with(&ctx, DialogButtons::Top, 2, true, None, Vec::new());
        assert_eq!(feature_frame_with(&ctx, DialogButtons::Top, 2, true, None, crate::test_support::key(key)).1, expected, "{key:?}");
    }
}

/// With no footer to show it, the compute status takes the title's place and the spinner the
/// icon's, until the feature is in; OK stays grey meanwhile, as in the footer.
#[test]
fn top_placement_shows_the_compute_status_in_the_title_row() {
    let ctx = crate::fonts::test_context();
    let vp = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(640.0, 480.0));
    let mut output = None;
    for _ in 0..3 {
        scenario::clear_handles();
        output = Some(crate::test_support::frame(&ctx, egui::RawInput { screen_rect: Some(vp), ..Default::default() }, |_| {
            DialogButtons::Top.publish(&ctx);
            let side = crate::frame::Side::Right;
            let anchor = egui::pos2(vp.right() - crate::ui_kit::SHELL_GAP, vp.top());
            let panel = Panel::new(
                "Fillet",
                egui::Id::unique("top_status"),
                anchor,
                vp,
                side,
                Panel::feature_frame(&ctx, crate::ui_kit::LayoutMode::Floating, side),
            );
            panel.window(&ctx).show(&ctx, |ui| {
                let heading = FeatureOptions {
                    icon: "fillet",
                    title: "Fillet",
                    drag_key: "top_status",
                    moved: egui::Vec2::ZERO,
                    docked: anchor,
                    apply_label: None,
                };
                panel.feature(ui, heading, true, true, |ui| {
                    text(ui, "Input", Text::Note("row"));
                    ComputeStatus { text: Some("Computing… 3 s".into()), busy: true, applying: true }
                });
            });
        }));
    }
    let texts: Vec<String> = output.iter().flat_map(crate::test_support::text_shapes).map(|shape| shape.galley.job.text.clone()).collect();
    assert!(texts.iter().any(|text| text == "Computing… 3 s"), "{texts:?}");
    assert!(!texts.iter().any(|text| text == "Fillet"), "the status stands in for the title: {texts:?}");
    assert_eq!(scenario::count_of("dialog:ok:enabled"), Some(0.0), "OK waits for the feature");
    assert!(scenario::rect_of("dialog:header").unwrap().contains_rect(scenario::rect_of("dialog:cancel").unwrap()));
}

/// With the buttons in the footer, the busy spinner sits in the footer's row: at its side inset
/// and on the buttons' centre line, not against the card's edge (it used to hang there).
#[test]
fn bottom_placement_puts_the_busy_spinner_in_the_footer_row() {
    let ctx = crate::fonts::test_context();
    let vp = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(640.0, 480.0));
    for _ in 0..3 {
        scenario::clear_handles();
        crate::test_support::frame(&ctx, egui::RawInput { screen_rect: Some(vp), ..Default::default() }, |_| {
            DialogButtons::Bottom.publish(&ctx);
            let side = crate::frame::Side::Right;
            let anchor = egui::pos2(vp.right() - crate::ui_kit::SHELL_GAP, vp.top());
            let panel = Panel::new(
                "Fillet",
                egui::Id::unique("bottom_status"),
                anchor,
                vp,
                side,
                Panel::feature_frame(&ctx, crate::ui_kit::LayoutMode::Floating, side),
            );
            panel.window(&ctx).show(&ctx, |ui| {
                let heading = FeatureOptions {
                    icon: "fillet",
                    title: "Fillet",
                    drag_key: "bottom_status",
                    moved: egui::Vec2::ZERO,
                    docked: anchor,
                    apply_label: None,
                };
                panel.feature(ui, heading, true, true, |ui| {
                    text(ui, "Input", Text::Note("row"));
                    ComputeStatus { text: Some("Computing… 3 s".into()), busy: true, applying: true }
                });
            });
        });
    }
    let spinner = scenario::rect_of("dialog:footer:spinner").expect("the spinner shows while busy");
    let actions = scenario::rect_of("dialog:footer:actions").unwrap();
    let ok = scenario::rect_of("dialog:ok").unwrap();
    assert!(actions.contains_rect(spinner), "{spinner:?} inside the footer row {actions:?}");
    assert!((spinner.left() - actions.left()).abs() < 0.5, "at the footer's inset: {spinner:?} {actions:?}");
    assert!((spinner.center().y - ok.center().y).abs() < 0.5, "on the buttons' line: {spinner:?} {ok:?}");
}

#[test]
fn top_placement_keeps_a_managers_fold_beside_the_actions() {
    let ctx = crate::fonts::test_context();
    let mut collapsed = false;
    for _ in 0..3 {
        feature_frame_with(&ctx, DialogButtons::Top, 4, true, Some(&mut collapsed), Vec::new());
    }
    let header = scenario::rect_of("dialog:header").unwrap();
    let fold = scenario::handle_at("dialog:collapse").unwrap();
    let cancel = scenario::rect_of("dialog:cancel").unwrap();
    assert!(header.contains(fold) && fold.x < cancel.left(), "{fold:?} left of {cancel:?}");
    assert!(scenario::handle_at("dialog:title").is_some_and(|at| at.x < fold.x));
    feature_frame_with(&ctx, DialogButtons::Top, 4, true, Some(&mut collapsed), crate::test_support::pointer(fold, true));
    feature_frame_with(&ctx, DialogButtons::Top, 4, true, Some(&mut collapsed), crate::test_support::pointer(fold, false));
    assert!(collapsed);
    assert_eq!(scenario::rect_of("dialog:cancel"), Some(cancel), "folding leaves the buttons in place");
}

#[test]
fn palette_scrolls_short_views_and_retains_its_status_and_action_handles() {
    for layout in [crate::ui_kit::LayoutMode::Docked, crate::ui_kit::LayoutMode::Floating] {
        for height in [125.0, 480.0] {
            let ctx = crate::fonts::test_context();
            let vp = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(640.0, height));
            for _ in 0..3 {
                scenario::clear_handles();
                crate::test_support::frame(&ctx, egui::RawInput { screen_rect: Some(vp), ..Default::default() }, |_| {
                    let frame = Panel::palette_frame(&ctx, layout);
                    let bottom = vp.bottom() - crate::ui_kit::SHELL_GAP;
                    let panel = Panel::new(
                        "Options",
                        egui::Id::unique("gallery_palette"),
                        egui::pos2(vp.right() - crate::ui_kit::SHELL_GAP, 0.0),
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
    for layout in [crate::ui_kit::LayoutMode::Floating, crate::ui_kit::LayoutMode::Docked] {
        for width in [240.0, 640.0, 1200.0] {
            let ctx = crate::fonts::test_context();
            let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 360.0));
            for _ in 0..3 {
                scenario::clear_handles();
                crate::test_support::frame(&ctx, egui::RawInput { screen_rect: Some(viewport), ..Default::default() }, |_| {
                    let side = crate::frame::Side::Right;
                    let anchor = viewport.right_top();
                    let panel =
                        Panel::new("Table", egui::Id::unique("wide"), anchor, viewport, side, Panel::feature_frame(&ctx, layout, side)).wide();
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
                    action = Window::new(egui::Id::unique("prompt_test"), "delete", "Delete")
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
                    let prompt = Window::new(egui::Id::unique(("prompt_escape", keep)), "settings", "Report").actions("Open", "Close", None);
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
                    Window::new(ui.scope_id().with("pages"), "settings", "Preferences").cards(3.0).show_pages(
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
                    Window::new(ui.scope_id().with("pages"), "settings", "Preferences").cards(3.0).show_pages(
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
fn page_window_is_as_opaque_as_every_other_window_in_both_themes() {
    // #9078: the Preferences window let the ribbon's labels show through its title bar and search field.
    fn window_fill(pages: bool) -> egui::Color32 {
        let ctx = crate::fonts::test_context();
        let list = [WindowPage {
            group: None,
            icon: "settings",
            label: "ui.general",
            handle: "test:page:general",
            scroll: true,
            draw: |ui, _: &mut ()| {
                text(ui, "General", Text::Inline);
            },
        }];
        let mut selected = 0;
        let mut output = None;
        for _ in 0..3 {
            scenario::clear_handles();
            output = Some(crate::test_support::frame(
                &ctx,
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 800.0))),
                    focused: true,
                    ..Default::default()
                },
                |ui| {
                    let window = Window::new(ui.scope_id().with("opaque"), "settings", "Preferences");
                    if pages {
                        window.cards(3.0).show_pages(ui.ctx(), &mut selected, &list, &mut (), |_| {});
                    } else {
                        window.show(ui.ctx(), true, |ui| {
                            text(ui, "Body", Text::Inline);
                        });
                    }
                },
            ));
        }
        let window = scenario::rect_of("dialog:window").expect("window rect published");
        let mut fill = None;
        for clipped in output.iter().flat_map(|o| &o.shapes) {
            crate::test_support::each_shape(&clipped.shape, &mut |shape| {
                if let egui::Shape::Rect(r) = shape
                    && fill.is_none()
                    && r.blur_width == 0.0
                    && (r.rect.min - window.min).abs().max_elem() <= 0.5
                    && (r.rect.max - window.max).abs().max_elem() <= 0.5
                {
                    fill = Some(r.fill);
                }
            });
        }
        fill.expect("the window frame is painted at the window's rect")
    }
    let previous = crate::theme::is_dark();
    for dark in [false, true] {
        crate::theme::set_dark(dark);
        let t = Tokens::get();
        assert_eq!(t.dialog_body.a(), 255, "{dark}: the window token itself is opaque");
        let plain = window_fill(false);
        assert_eq!(plain, t.dialog_body, "{dark}: a plain window is the opaque dialog body");
        let page = window_fill(true);
        assert_eq!(page.a(), 255, "{dark}: the Preferences page window lets nothing show through");
        assert_eq!(page, plain, "{dark}: the page window has the same fill as every other window");
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
                    action = Confirm::new(ui.scope_id().with("modal"), "status_warning", "Save changes?")
                        .tone(Tone::Warning)
                        .actions("Save", "Cancel", Some("Don't Save"))
                        .secondary_left()
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
fn save_prompt_frames_dont_save_and_clicking_it_discards_for_close_and_quit() {
    for quit in [false, true] {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.run("solid.box", serde_json::json!({"length": 10, "width": 10, "height": 10})).unwrap();
        if quit {
            crate::documents::request_quit(&mut h.app);
        } else {
            let active = h.app.docs.active;
            crate::documents::close(&mut h.app, active, false);
        }
        h.ctx = crate::fonts::test_context();
        h.ctx.enable_accesskit();
        let mut textures = std::collections::HashMap::new();
        let mut output = egui::FullOutput::default();
        for _ in 0..3 {
            output = h.ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0))),
                    focused: true,
                    ..Default::default()
                },
                |ui| {
                    h.app.logic(ui.ctx());
                    h.app.ui(ui);
                },
            );
            super::super::checks::tests::textures_update(&mut textures, &output.textures_delta);
            output.textures_delta.clear();
        }
        h.output = output.platform_output.clone();
        let discard = scenario::rect_of("dialog:apply").unwrap();
        assert!(discard.contains(scenario::handle_at("dialog:Don't Save").unwrap()));
        assert!(
            h.output
                .accesskit_update
                .as_ref()
                .unwrap()
                .nodes
                .iter()
                .any(|(_, node)| { node.label() == Some("Don't Save") && node.role() == egui::accesskit::Role::Button }),
            "discard remains an accessible button"
        );
        let t = crate::theme::Tokens::get();
        let mut framed = false;
        for shape in &output.shapes {
            crate::test_support::each_shape(&shape.shape, &mut |s| {
                framed |= matches!(s, egui::Shape::Rect(r) if r.rect == discard
                    && r.fill == t.control_fill && r.stroke.width > 0.0 && r.stroke.color == t.control_border);
            });
        }
        assert!(framed, "Don't Save needs a visible secondary button frame before hover or selection");
        if let Ok(dir) = std::env::var("SOLIDCRAFT_SAVE_PROMPT_EVIDENCE") {
            let bounds = scenario::rect_of("dialog:header").unwrap().union(scenario::rect_of("dialog:footer").unwrap()).expand(t.space_24);
            let image = super::super::checks::tests::raster(&h.ctx, output.shapes, output.pixels_per_point, bounds, &textures);
            std::fs::create_dir_all(&dir).unwrap();
            image.save(std::path::Path::new(&dir).join(if quit { "save-quit.png" } else { "save-close.png" })).unwrap();
        }
        h.step(&serde_json::json!({"click": {"handle": "dialog:Don't Save"}})).unwrap();
        h.frames(2);
        assert!(h.app.docs.closing.is_none());
        assert!(!h.app.session.is_dirty(), "the changes were discarded");
        assert_eq!(h.app.quit_requested, quit, "continue the requested close or quit");
        if !quit {
            assert!(h.app.session.doc.features.is_empty(), "closing the last tab leaves a blank design");
        }
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
                    Confirm::new(ui.scope_id().with("delete"), "delete", "Delete").tone(Tone::Danger).actions("Delete", "Cancel", None).show(
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
            let header = scenario::rect_of("dialog:header").unwrap();
            let search = scenario::rect_of("prefs:search:frame").unwrap();
            assert!((header.height() - 40.0).abs() < 0.5);
            assert!(header.contains_rect(search), "{dark}: {page}: search stays inside the titlebar");
            assert!((search.width() - 230.0).abs() < 0.5 && (search.height() - 26.0).abs() < 0.5);
            assert!(search.left() > header.center().x);
            for action in ["prefs:ok", "prefs:apply", "prefs:cancel"] {
                assert!(window.contains_rect(scenario::rect_of(action).unwrap()), "{dark}: {page}: {action}");
            }
            assert!(scenario::rect_of("prefs:page_body").is_some());
            assert!(h.widget(h.app.tr("prefs.restore_page_defaults")).is_some());
            let footer = scenario::rect_of("dialog:pages_footer").expect("pinned profile footer");
            assert!(nav.contains_rect(footer));
            let profile_card = scenario::rect_of("prefs:profile:sidebar_card").expect("bordered profile card");
            assert!(footer.contains_rect(profile_card));
            assert!((profile_card.height() - 48.0).abs() < 0.5);
            assert_eq!(profile_card, scenario::rect_of("prefs:profile").expect("the whole card is the real switcher"));
            let icon = scenario::rect_of("prefs:profile:icon").unwrap();
            let label = scenario::rect_of("prefs:profile:label").unwrap();
            let name = scenario::rect_of("prefs:profile:name").unwrap();
            let chevron = scenario::rect_of("prefs:profile:chevron").unwrap();
            for part in [icon, label, name, chevron] {
                assert!(profile_card.contains_rect(part));
            }
            assert!(icon.right() < label.left() && label.bottom() <= name.top() && name.right() < chevron.left());
            assert!((label.left() - name.left()).abs() < 0.5);
            assert!((icon.center().y - profile_card.center().y).abs() < 0.5);
            let tagged = [
                crate::prefs::PAGE_GENERAL,
                crate::prefs::PAGE_APPEARANCE,
                crate::prefs::PAGE_LAYOUT,
                crate::prefs::PAGE_SKETCH,
                crate::prefs::PAGE_NAVIGATION,
                crate::prefs::PAGE_SHORTCUTS,
            ]
            .contains(&index);
            assert_eq!(scenario::rect_of("prefs:in_profile").is_some(), tagged, "{page}");
            if index == crate::prefs::PAGE_GENERAL {
                let tag = scenario::rect_of("prefs:in_profile").unwrap();
                let body = scenario::rect_of("prefs:page_body").unwrap();
                assert!((tag.height() - 22.0).abs() < 0.5);
                assert!((tag.right() - body.right()).abs() < 0.5, "{dark}: the profile tag aligns to the page's right edge");
            }
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
                let window = Window::new(ui.scope_id().with("optional_footer"), "settings", "Preferences").actions("", "", None);
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
    use crate::{frame::Side, ui_kit::LayoutMode};
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
            Surface::ViewportStatus,
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
            assert!(!Window::new(ui.scope_id().with("required_recovery"), "restore", "Recover unsaved work?").required().show_body(ui.ctx(), |ui| {
                button(ui, "Recover", Button::Normal);
                button(ui, "Discard", Button::Normal);
            }));
        });
    }
    assert!(scenario::handle_at("dialog:close").is_none(), "recovery cannot be dismissed through the header");
    assert!(scenario::rect_of("dialog:header").is_some());
    assert!(scenario::rect_of("dialog:footer").is_none());
}

/// #9472: a card first laid out while the page is still tiny (a browser canvas not sized yet)
/// grows its body once the page has its size, so the choice stays visible on every page size and
/// scale. The browser's recovery card and the desktop one (with a footer) share this body.
#[test]
fn window_body_buttons_stay_visible_after_the_page_grows_from_nothing() {
    for desktop in [false, true] {
        for ppp in [1.0, 1.5, 2.0] {
            for size in [egui::vec2(2992.0, 1692.0), egui::vec2(1496.0, 846.0), egui::vec2(800.0, 600.0), egui::vec2(320.0, 200.0)] {
                let ctx = crate::fonts::test_context();
                let mut buttons = Vec::new();
                for page in [egui::Vec2::ZERO, egui::vec2(1.0, 1.0), egui::vec2(300.0, 150.0), size, size, size, size] {
                    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, page);
                    let viewport = egui::ViewportInfo { native_pixels_per_point: Some(ppp), ..Default::default() };
                    let input = egui::RawInput {
                        screen_rect: Some(screen),
                        focused: true,
                        viewports: std::iter::once((egui::ViewportId::ROOT, viewport)).collect(),
                        ..Default::default()
                    };
                    buttons.clear();
                    crate::test_support::frame(&ctx, input, |ui| {
                        let window = Window::new(egui::Id::unique("sc_browser_recovery"), "restore", "Recover unsaved work?");
                        let body = |ui: &mut Ui| {
                            ui.label("“Bracket” had unsaved changes when this page last closed.");
                            ui.horizontal(|ui| {
                                for label in ["Recover", "Discard"] {
                                    let rect = button(ui, label, Button::Normal).rect;
                                    buttons.push((label, rect, ui.clip_rect()));
                                }
                            });
                        };
                        if desktop {
                            window.actions("", "Later", None).show_with_left(ui.ctx(), true, body, |_| {});
                        } else {
                            window.required().show_body(ui.ctx(), body);
                        }
                    });
                }
                let screen = ctx.content_rect();
                assert_eq!(buttons.len(), 2, "desktop {desktop}, {size:?} at {ppp}");
                for (label, rect, clip) in &buttons {
                    assert!(rect.height() > 0.0 && rect.width() > 0.0, "{label}: desktop {desktop}, {size:?} at {ppp}: {rect:?}");
                    assert!(
                        clip.contains_rect(*rect) && screen.contains_rect(*rect),
                        "{label} hidden: desktop {desktop}, {size:?} at {ppp}: {rect:?} clipped to {clip:?} on {screen:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn adjoining_compact_frames_share_one_stroke_and_only_exposed_corners_round() {
    use crate::ui_kit::DockEdges;
    use crate::{frame::Side, ui_kit::LayoutMode};
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
    crate::ui_kit::LayoutMode::Docked.publish(&ctx);
    let t = Tokens::get();
    let container = Container::new(&ctx, Surface::Feature);
    let frame = container.frame(&ctx, t.dialog_body, crate::frame::Side::Right);
    assert_eq!(container.tool_frame(frame, false).corner_radius, egui::CornerRadius::same(t.radius_lg));
    assert_eq!(container.tool_frame(frame, true).corner_radius, egui::CornerRadius { nw: t.radius_lg, ne: 0, sw: t.radius_lg, se: 0 });
}

#[test]
fn theme_shape_changes_preserve_panel_geometry_and_dock_contacts() {
    use crate::{frame::Side, ui_kit::LayoutMode};
    let ctx = crate::fonts::test_context();
    let mut results = Vec::new();
    for name in ["solidcraft-95", "blueprint", "cupertino"] {
        let t = crate::theme_files::resolve(crate::theme_files::find(name, &[]).unwrap());
        let _theme = crate::theme::scoped_tokens(t);
        for surface in [Surface::Panel, Surface::Feature, Surface::Window, Surface::Popup, Surface::Navigation, Surface::Toast] {
            crate::theme::apply(&ctx);
            let floating = Container::for_layout(LayoutMode::Floating, surface).frame(&ctx, t.panel, Side::Left);
            let bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(240.0, 180.0));
            results.push((name, surface, floating.corner_radius, floating.outer_rect(bounds), t));
        }
        let docked = Container::for_layout(LayoutMode::Docked, Surface::Panel)
            .joined(DockEdges { left: true, top: true, ..Default::default() })
            .frame(&ctx, t.panel, Side::Left);
        assert_eq!(docked.corner_radius, egui::CornerRadius { nw: 0, ne: 0, sw: 0, se: t.radius_lg });
    }
    for (name, surface, radius, bounds, t) in &results {
        let expected = match surface {
            Surface::Panel => t.radius_lg,
            Surface::Feature | Surface::Window => t.radius_xl,
            _ => t.radius_md,
        };
        assert_eq!(*radius, egui::CornerRadius::same(expected), "{name}: {surface:?}");
        assert_eq!(*bounds, results.iter().find(|row| row.1 == *surface).unwrap().3, "shape must not change layout");
    }
}

#[test]
fn border_hierarchy_reaches_hosts_sections_controls_and_inset_without_new_targets() {
    use crate::theme::{Borders, InsetBorder};
    use egui::{Color32, Stroke};
    for dark in [false, true] {
        let mut t = if dark { Tokens::dark() } else { Tokens::light() };
        t.borders = Borders {
            primary: Some(Stroke::new(4.0, Color32::WHITE)),
            secondary: Some(Stroke::new(2.0, Color32::RED)),
            secondary_dotted: false,
            tertiary: Some(Stroke::new(0.5, Color32::GREEN)),
            container: None,
            bevel: None,
            control: Some(Stroke::new(1.5, Color32::BLUE)),
            primary_inset: Some(InsetBorder { stroke: Stroke::new(0.75, Color32::YELLOW), gap: 3.0 }),
        };
        let _restore = crate::theme::scoped_tokens(t);
        let ctx = crate::fonts::test_context();
        let mut value = "12 mm".to_string();
        let output = crate::test_support::frame(&ctx, egui::RawInput::default(), |ui| {
            let container = Container::for_layout(crate::ui_kit::LayoutMode::Floating, Surface::Panel);
            let frame = container.frame(ui.ctx(), t.panel, crate::frame::Side::Left).inner_margin(12);
            let bounds = ui.max_rect();
            container.show_frame(ui, frame, bounds, |ui| {
                ui.set_width(280.0);
                scope(ui, |ui| {
                    header(ui, "box", "Box", || {});
                    row(ui, "Width", |ui| {
                        field(ui, &mut value);
                    });
                    ui.separator();
                });
            });
        });
        let mut found = [false; 5];
        for shape in &output.shapes {
            crate::test_support::each_shape(&shape.shape, &mut |shape| {
                let stroke = match shape {
                    egui::Shape::Rect(rect) => Some(rect.stroke),
                    egui::Shape::LineSegment { stroke, .. } => Some(*stroke),
                    _ => None,
                };
                if let Some(stroke) = stroke {
                    for (index, expected) in [
                        t.borders.primary.unwrap(),
                        t.borders.secondary.unwrap(),
                        t.borders.tertiary.unwrap(),
                        t.borders.control.unwrap(),
                        t.borders.primary_inset.unwrap().stroke,
                    ]
                    .iter()
                    .enumerate()
                    {
                        found[index] |= stroke == *expected;
                    }
                }
            });
        }
        assert!(found.into_iter().all(|v| v), "missing role: {found:?}");
        for layout in [crate::ui_kit::LayoutMode::Floating, crate::ui_kit::LayoutMode::Docked] {
            for surface in [
                Surface::Panel,
                Surface::Chrome,
                Surface::Feature,
                Surface::Palette,
                Surface::Window,
                Surface::Popup,
                Surface::Toast,
                Surface::Navigation,
            ] {
                let frame = Container::for_layout(layout, surface).frame(&ctx, t.panel, crate::frame::Side::Left);
                assert_eq!(frame.stroke, t.borders.primary.unwrap(), "{layout:?} {surface:?}");
                assert!(frame.total_margin().sum().is_finite());
            }
        }
    }
}

#[test]
fn inset_border_skips_tiny_or_invalid_surfaces_and_keeps_dock_contacts_square() {
    let mut t = Tokens::dark();
    t.borders.primary = Some(egui::Stroke::new(4.0, egui::Color32::WHITE));
    t.borders.primary_inset = Some(crate::theme::InsetBorder { stroke: egui::Stroke::new(4.0, egui::Color32::YELLOW), gap: 12.0 });
    let _restore = crate::theme::scoped_tokens(t);
    let ctx = crate::fonts::test_context();
    let output = crate::test_support::frame(&ctx, egui::RawInput::default(), |ui| {
        let frame = Container::for_layout(crate::ui_kit::LayoutMode::Docked, Surface::Panel).joined(DockEdges::ALL).frame(
            &ctx,
            t.panel,
            crate::frame::Side::Left,
        );
        assert_eq!(frame.corner_radius, egui::CornerRadius::ZERO);
        assert_eq!(frame.outer_margin.left, -4);
        for rect in [egui::Rect::from_min_size(egui::pos2(20.0, 20.0), egui::vec2(20.0, 20.0)), egui::Rect::NOTHING] {
            Container::paint_inset(ui.painter(), rect, frame);
        }
    });
    for shape in &output.shapes {
        crate::test_support::each_shape(&shape.shape, &mut |s| {
            if let egui::Shape::Rect(rect) = s {
                assert_ne!(rect.stroke.color, egui::Color32::YELLOW);
            }
        });
    }
}

#[test]
fn menu_companion_is_visible_in_frame_padding_and_bounded_by_its_surface() {
    let mut t = Tokens::dark();
    t.borders.primary = Some(egui::Stroke::new(2.0, egui::Color32::WHITE));
    t.borders.primary_inset = Some(crate::theme::InsetBorder { stroke: egui::Stroke::new(0.75, egui::Color32::YELLOW), gap: 3.0 });
    let _restore = crate::theme::scoped_tokens(t);
    let ctx = crate::fonts::test_context();
    let mut surface = egui::Rect::NOTHING;
    let output = crate::test_support::frame(&ctx, egui::RawInput::default(), |ui| {
        surface = Menu::show_frame(ui, Menu::frame(), |ui| {
            ui.set_width(200.0);
            ui.label("New Design");
            Menu::separator(ui);
            ui.label("Open");
        })
        .response
        .rect;
    });
    let mut found = false;
    for clipped in &output.shapes {
        crate::test_support::each_shape(&clipped.shape, &mut |shape| {
            if let egui::Shape::Rect(rect) = shape
                && rect.stroke.color == egui::Color32::YELLOW
            {
                found = true;
                assert!(surface.contains_rect(rect.rect));
                assert!(clipped.clip_rect.contains_rect(rect.rect));
                assert!((rect.rect.left() - surface.left() - 5.0).abs() < 0.01);
            }
        });
    }
    assert!(found, "the companion must be painted outside the menu content clip");
}

#[test]
fn bevel_lighting_reverses_for_fields_and_pressed_buttons_without_changing_geometry() {
    use egui::{Color32, Pos2, Rect};
    let mut t = Tokens::light();
    let bevel =
        crate::theme::Bevel { width: 1.0, highlight: Color32::WHITE, light: Color32::LIGHT_GRAY, shadow: Color32::GRAY, dark_shadow: Color32::BLACK };
    t.borders.bevel = Some(bevel);
    t.control_radius = 0;
    let _restore = crate::theme::scoped_tokens(t);
    let ctx = crate::fonts::test_context();
    let mut button_rect = Rect::NOTHING;
    let mut field_rect = Rect::NOTHING;
    let mut text = "12 mm".to_string();
    let mut draw = |ui: &mut Ui| {
        ui.set_width(240.0);
        scope(ui, |ui| {
            button_rect = button(ui, "Review", Button::Normal).rect;
            field_rect = cell_editor(ui, egui::Id::unique("bevel-field"), &mut text, "", false).rect;
        });
    };
    fn top_color(output: &egui::FullOutput, rect: Rect, inset: f32) -> Color32 {
        output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::LineSegment { points: [a, b], stroke }
                    if (a.y - rect.top() - inset).abs() < 0.01 && (b.y - a.y).abs() < 0.01 && a.x >= rect.left() && b.x <= rect.right() =>
                {
                    Some(stroke.color)
                }
                _ => None,
            })
            .expect("horizontal bevel edge")
    }
    let raised = crate::test_support::frame(&ctx, egui::RawInput::default(), &mut draw);
    assert_eq!(top_color(&raised, button_rect, 0.5), bevel.highlight);
    assert_eq!(top_color(&raised, field_rect, 1.5), bevel.shadow);
    let original_rect = button_rect;
    let mut draw = |ui: &mut Ui| {
        ui.set_width(240.0);
        scope(ui, |ui| {
            button_rect = button(ui, "Review", Button::Normal).rect;
            cell_editor(ui, egui::Id::unique("bevel-field"), &mut text, "", false);
        });
    };
    let down = crate::test_support::frame(
        &ctx,
        egui::RawInput { events: crate::test_support::pointer(original_rect.center(), true), ..Default::default() },
        &mut draw,
    );
    assert_eq!(button_rect, original_rect);
    assert_eq!(top_color(&down, button_rect, 0.5), bevel.shadow);
    let tiny = Rect::from_min_size(Pos2::ZERO, egui::vec2(2.0, 2.0));
    let output = crate::test_support::frame(&ctx, egui::RawInput::default(), |ui| bevel.paint(ui.painter(), tiny, 0, false, true));
    assert!(!output.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::LineSegment { .. })));
}

/// A left-anchored panel in a view that came out inside out keeps an anchor instead of
/// panicking in `f32::clamp`, as the layout beside it did in #9492.
#[test]
fn a_left_panel_in_an_inside_out_view_keeps_an_anchor() {
    let ctx = crate::fonts::test_context();
    let inside_out = egui::Rect::from_min_max(egui::pos2(264.1875, 40.0), egui::pos2(159.0, 600.0));
    let panel = super::Panel::new(
        "Panel",
        egui::Id::unique("inside_out_panel"),
        egui::pos2(200.0, 80.0),
        inside_out,
        crate::frame::Side::Left,
        egui::Frame::NONE,
    );
    let anchor = panel.bounded_anchor(&ctx);
    assert!(anchor.x.is_finite() && anchor.y.is_finite(), "{anchor:?}");
}

#[test]
fn page_sidebar_footer_stays_visible_when_the_page_list_scrolls() {
    for theme in ["light", "dark"] {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.preferences.theme = theme.into();
        h.call("ui.resize", serde_json::json!({"width":1200,"height":620}));
        crate::prefs::open_at(&mut h.app, crate::prefs::PAGE_GENERAL);
        h.frames(6);
        let footer = scenario::rect_of("dialog:pages_footer").unwrap();
        let nav = scenario::rect_of("dialog:pages").unwrap();
        h.frame_with(vec![
            egui::Event::PointerMoved(egui::pos2(nav.center().x, nav.top() + 30.0)),
            egui::Event::MouseWheel {
                source: egui::MouseWheelSource::Unknown,
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, -1000.0),
                modifiers: egui::Modifiers::NONE,
                phase: egui::TouchPhase::Move,
            },
        ]);
        h.frames(60);
        assert_eq!(scenario::rect_of("dialog:pages_footer"), Some(footer));
        assert!(footer.contains_rect(scenario::rect_of("prefs:profile").unwrap()));
        assert!(nav.contains_rect(scenario::rect_of("prefs:page:Plugins").unwrap()), "last page is reachable in {theme}");
    }
}

#[test]
fn host_surface_fallback_paints_readable_plain_labels_without_changing_body_layout() {
    use solidcraft_engine::panels::{ContrastOutcome, PanelSurface};
    let bounds = egui::Rect::from_min_size(egui::pos2(40.0, 40.0), egui::vec2(320.0, 180.0));
    for floating in [false, true] {
        for fallback in [false, true] {
            let ctx = crate::fonts::test_context();
            let mut authored = Tokens::get();
            authored.panel = egui::Color32::WHITE;
            authored.text = egui::Color32::BLACK;
            authored.text_dim = egui::Color32::BLACK;
            authored.form_ink = egui::Color32::BLACK;
            authored.form_dim = egui::Color32::BLACK;
            authored.dialog_label = egui::Color32::BLACK;
            if fallback {
                authored.text = authored.panel;
            }
            let _authored = crate::theme::scoped_panel_tokens(authored);
            let style = crate::panel_runtime::HostSurfaceStyle::capture(&ctx, false, Some(0.0), None);
            let appearance = style.appearance(PanelSurface::Translucent, floating);
            let failed = matches!(appearance.contrast, ContrastOutcome::RetainReadableFallback { .. });
            assert_eq!(failed, fallback, "exercise both authored paint and deliberately unreadable fallback");
            let palette = failed.then(|| style.palette(appearance));
            let font = egui::FontId::monospace(19.0);
            // Floating Areas create a fresh Ui from the context style; they do
            // not inherit a caller's local Ui overrides. Set the common baseline
            // before the sizing pass so both existing host paths inherit it.
            ctx.global_style_mut(|style| {
                style.override_font_id = Some(font.clone());
                style.spacing.item_spacing = egui::vec2(13.0, 17.0);
                style.wrap_mode = Some(egui::TextWrapMode::Wrap);
                style.visuals.override_text_color = Some(authored.panel);
            });
            if floating {
                crate::test_support::frame(&ctx, egui::RawInput::default(), |ui| {
                    panel_host_surface(ui, egui::Id::unique("captured-fill"), bounds, floating, appearance.fill, palette, |_, _| {});
                });
            }
            let mut expected_ink = egui::Color32::TRANSPARENT;
            let output = crate::test_support::frame(&ctx, egui::RawInput::default(), |ui| {
                let parent_visuals = ui.visuals().clone();
                let parent_spacing = ui.spacing().item_spacing;
                let parent_fonts = ui.style().text_styles.clone();
                let parent_wrap = ui.style().wrap_mode;
                expected_ink = palette.map_or(authored.panel, |tokens| tokens.text);
                panel_host_surface(ui, egui::Id::unique("captured-fill"), bounds, floating, appearance.fill, palette, |ui, clip| {
                    assert_eq!(Tokens::get(), palette.copied().unwrap_or(authored));
                    assert_eq!(ui.spacing().item_spacing, parent_spacing);
                    assert_eq!(ui.style().text_styles, parent_fonts);
                    assert_eq!(ui.style().override_font_id, Some(font.clone()));
                    assert_eq!(ui.style().wrap_mode, parent_wrap);
                    if palette.is_none() {
                        assert_eq!(ui.visuals(), &parent_visuals, "normal bodies inherit unchanged visuals");
                    }
                    assert!(bounds.contains_rect(clip));
                    ui.label("Readable captured panel");
                });
                assert_eq!(Tokens::get(), authored);
                assert_eq!(ui.visuals(), &parent_visuals, "fallback visuals restore immediately");
                assert_eq!(ui.spacing().item_spacing, parent_spacing);
                assert_eq!(ui.style().text_styles, parent_fonts);
            });
            let (mut backgrounds, mut labels) = (0, 0);
            for shape in &output.shapes {
                crate::test_support::each_shape(&shape.shape, &mut |shape| match shape {
                    egui::Shape::Rect(rect) if rect.rect == bounds && rect.fill != egui::Color32::TRANSPARENT => {
                        assert_eq!(rect.fill, appearance.fill, "actual paint uses the captured fill");
                        backgrounds += 1;
                    }
                    egui::Shape::Text(text) if text.galley.job.text == "Readable captured panel" => {
                        for section in &text.galley.job.sections {
                            let ink = text.override_text_color.unwrap_or_else(|| {
                                if section.format.color == egui::Color32::PLACEHOLDER { text.fallback_color } else { section.format.color }
                            });
                            assert_eq!(ink, expected_ink, "the actual plain-label galley uses readable fallback ink");
                            assert_eq!(section.format.font_id, font, "fallback preserves the body's chosen font");
                        }
                        labels += 1;
                    }
                    _ => {}
                });
            }
            assert!(backgrounds > 0 && labels > 0, "inspect real background and label paint");
            assert_eq!(Tokens::get(), authored);
        }
    }
}
