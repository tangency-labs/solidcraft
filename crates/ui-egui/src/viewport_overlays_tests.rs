use super::*;
use crate::{scenario::Harness, shell::LayoutMode};

#[test]
fn viewport_status_specimens_clear_and_open_keys_without_engine_changes() {
    let mut h = Harness::new();
    h.call("ui.status_fixture", json!({"preset":"row"}));
    h.frames(4);
    let zebra = crate::scenario::rect_of("viewport_status:zebra:item").unwrap();
    let section = crate::scenario::rect_of("viewport_status:section:item").unwrap();
    assert!(zebra.right() < section.left());
    assert!((zebra.center().y - section.center().y).abs() < 1.0, "single row: {zebra:?}, {section:?}");
    assert!(crate::scenario::rect_of("viewport_status:zebra:key").is_none());
    assert!(h.app.session.analysis.is_none());
    h.step(&json!({"click":{"handle":"viewport_status:zebra:close"}})).unwrap();
    h.frames(2);
    assert!(crate::scenario::rect_of("viewport_status:zebra:item").is_none());
    assert!(crate::scenario::rect_of("viewport_status:section:item").is_some());
    h.call("ui.status_fixture", json!({"preset":"key"}));
    h.frames(4);
    h.step(&json!({"click":{"handle":"viewport_status:draft:key"}})).unwrap();
    h.frames(3);
    let item = crate::scenario::rect_of("viewport_status:draft:item").unwrap();
    let card = crate::scenario::rect_of("viewport_status:card").unwrap();
    assert!(card.top() > item.bottom());
    assert!((card.left() - item.left()).abs() < 1.0);
    h.step(&json!({"key":"Enter"})).unwrap();
    h.frames(2);
    assert!(crate::scenario::rect_of("viewport_status:card").is_none(), "focused Ghost key toggles with Enter");
    h.step(&json!({"key":"Space"})).unwrap();
    h.frames(2);
    assert!(crate::scenario::rect_of("viewport_status:card").is_some(), "focused Ghost key toggles with Space");
}

#[test]
fn viewport_status_kinds_keep_shape_outline_and_typography_across_themes() {
    use crate::ui_kit::{StatusItem, StatusKind, StatusPresentation, viewport_status_item};
    let previous = (crate::theme::is_dark(), crate::theme::vision());
    assert_ne!(StatusKind::Mode.icon(), StatusKind::Result.icon());
    assert_ne!(StatusKind::Result.icon(), StatusKind::Attention.icon());
    for dark in [false, true] {
        for vision in crate::theme::ColorVision::ALL {
            crate::theme::set_dark(dark);
            crate::theme::set_vision(vision);
            let t = Tokens::get();
            for (kind, outline) in [(StatusKind::Mode, t.accent), (StatusKind::Result, t.border), (StatusKind::Attention, t.warning)] {
                let ctx = crate::fonts::test_context();
                let item = StatusItem { id: "test".into(), kind, name: "Draft".into(), setting: "Z · 1°".into(), has_key: true, editable: false };
                let output = crate::test_support::frame(&ctx, egui::RawInput::default(), |ui| {
                    viewport_status_item(ui, &item, &mut StatusPresentation::default());
                });
                let (mut border, mut mono, mut name) = (false, false, false);
                for shape in &output.shapes {
                    crate::test_support::each_shape(&shape.shape, &mut |shape| match shape {
                        Shape::Rect(rect) if rect.fill == t.overlay && rect.stroke.color == outline => border = true,
                        Shape::Text(text) if text.galley.job.text == item.setting => {
                            mono = text.galley.job.sections.first().is_some_and(|s| s.format.font_id.family == egui::FontFamily::Monospace);
                        }
                        Shape::Text(text) if text.galley.job.text == item.name => {
                            name = text.galley.job.sections.first().is_some_and(|s| s.format.font_id == crate::fonts::semibold(t.font_label));
                        }
                        _ => {}
                    });
                }
                assert!(border && mono && name, "{dark:?} {vision:?} {kind:?}: outline {border}, mono {mono}, name {name}");
            }
        }
    }
    crate::theme::set_dark(previous.0);
    crate::theme::set_vision(previous.1);
}

#[test]
fn viewport_status_every_part_is_centred_within_its_pill_in_every_theme() {
    use crate::ui_kit::{StatusItem, StatusKind, StatusPresentation, viewport_status_item};
    let previous = (crate::theme::is_dark(), crate::theme::vision());
    for dark in [false, true] {
        for vision in crate::theme::ColorVision::ALL {
            crate::theme::set_dark(dark);
            crate::theme::set_vision(vision);
            for kind in [StatusKind::Mode, StatusKind::Result, StatusKind::Attention] {
                for (name, setting, has_key, editable) in [
                    ("Zebra", "12 stripes", false, false),
                    ("A longer surface analysis name", "R 1234.5 mm", true, false),
                    ("Mass centre", "", false, false),
                    ("Draft", "Z · 1°", true, true),
                ] {
                    let ctx = crate::fonts::test_context();
                    let item = StatusItem { id: "centred".into(), kind, name: name.into(), setting: setting.into(), has_key, editable };
                    let output = crate::test_support::frame(&ctx, egui::RawInput::default(), |ui| {
                        viewport_status_item(ui, &item, &mut StatusPresentation::default());
                    });
                    let pill = crate::scenario::rect_of("viewport_status:centred:item").unwrap();
                    let centre = pill.center().y;
                    let mut parts = vec!["icon", "name", "close", "close_glyph"];
                    if !setting.is_empty() {
                        parts.push("setting");
                    }
                    if has_key {
                        parts.extend(["key", "key_glyph"]);
                    }
                    for part in parts {
                        let rect = crate::scenario::rect_of(&format!("viewport_status:centred:{part}")).unwrap();
                        assert!((rect.center().y - centre).abs() <= 0.5, "{dark:?} {vision:?} {kind:?} {name} {part}: {rect:?}, pill {pill:?}");
                        assert!(pill.contains_rect(rect), "{part} stays inside its pill");
                    }
                    let mut text_centres = Vec::new();
                    for shape in &output.shapes {
                        crate::test_support::each_shape(&shape.shape, &mut |shape| {
                            if let Shape::Text(text) = shape
                                && (text.galley.job.text == name || (!setting.is_empty() && text.galley.job.text == setting))
                            {
                                text_centres.push(text.pos.y + text.galley.size().y / 2.0);
                            }
                        });
                    }
                    assert_eq!(text_centres.len(), if setting.is_empty() { 1 } else { 2 });
                    assert!(
                        text_centres.iter().all(|y| (*y - centre).abs() <= 0.5),
                        "painted text must share the pill centre: {text_centres:?}, {pill:?}"
                    );
                }
            }
        }
    }
    crate::theme::set_dark(previous.0);
    crate::theme::set_vision(previous.1);
}

// These tests inspect shapes without submitting their texture uploads to a renderer.
#[test]
fn viewport_status_close_hover_has_a_theme_fill_and_native_space_action_in_every_mode() {
    use crate::ui_kit::{StatusAction, StatusItem, StatusKind, StatusPresentation, viewport_status_item};

    fn paint(
        ctx: &egui::Context,
        item: &StatusItem,
        state: &mut StatusPresentation,
        events: Vec<egui::Event>,
    ) -> (egui::FullOutput, Option<StatusAction>) {
        let mut action = None;
        let mut output = crate::test_support::frame(ctx, egui::RawInput { events, focused: true, ..Default::default() }, |ui| {
            action = viewport_status_item(ui, item, state).1;
        });
        output.textures_delta.clear();
        (output, action)
    }

    fn has_hover_fill(output: &egui::FullOutput, target: Rect, t: Tokens) -> bool {
        let mut found = false;
        for shape in &output.shapes {
            crate::test_support::each_shape(&shape.shape, &mut |shape| {
                if let Shape::Rect(rect) = shape {
                    found |= rect.rect == target && rect.fill == t.hover && rect.corner_radius == t.control_radius.into();
                }
            });
        }
        found
    }

    let previous = (crate::theme::is_dark(), crate::theme::vision());
    for dark in [false, true] {
        for vision in crate::theme::ColorVision::ALL {
            crate::theme::set_dark(dark);
            crate::theme::set_vision(vision);
            let t = Tokens::get();
            let ctx = crate::fonts::test_context();
            let item = StatusItem {
                id: "hover".into(),
                kind: StatusKind::Mode,
                name: "Zebra".into(),
                setting: "12 stripes".into(),
                has_key: false,
                editable: false,
            };
            let mut state = StatusPresentation::default();
            let (rest, action) = paint(&ctx, &item, &mut state, vec![]);
            let close = crate::scenario::rect_of("viewport_status:hover:close").unwrap();
            assert!(action.is_none());
            assert!(!has_hover_fill(&rest, close, t), "rest must stay quiet: {dark:?} {vision:?}");
            let (hover, action) = paint(&ctx, &item, &mut state, vec![egui::Event::PointerMoved(close.center())]);
            assert!(action.is_none(), "hover cannot clear the item");
            assert!(has_hover_fill(&hover, close, t), "close must expose its token hover target: {dark:?} {vision:?}");
            assert_eq!(hover.platform_output.cursor_icon, egui::CursorIcon::PointingHand);
            paint(&ctx, &item, &mut state, crate::test_support::pointer(close.center(), true));
            let (_, clicked) = paint(&ctx, &item, &mut state, crate::test_support::pointer(close.center(), false));
            assert_eq!(clicked, Some(StatusAction::Clear(item.id.clone())));
            let (_, keyboard) = paint(&ctx, &item, &mut state, crate::test_support::key(egui::Key::Space));
            assert_eq!(keyboard, Some(StatusAction::Clear(item.id.clone())), "hover decoration must preserve native Space activation");
        }
    }
    crate::theme::set_dark(previous.0);
    crate::theme::set_vision(previous.1);
}

fn draw(ctx: &egui::Context, paint: impl FnMut(&mut egui::Ui)) -> egui::FullOutput {
    let mut output = ctx.run_ui(egui::RawInput::default(), paint);
    output.textures_delta.clear();
    output
}

fn sketch_stroke(shape: &Shape, color: Color32) -> bool {
    match shape {
        Shape::LineSegment { stroke, .. } => stroke.color == color,
        Shape::Path(path) => matches!(path.stroke.color, egui::epaint::ColorMode::Solid(c) if c == color),
        Shape::Vec(shapes) => shapes.iter().any(|s| sketch_stroke(s, color)),
        _ => false,
    }
}

#[test]
fn compass_keeps_signed_targets_hover_and_clicks_at_both_scales() {
    for mode in [LayoutMode::Floating, LayoutMode::Docked] {
        for scale in [1.0, 2.0] {
            let mut h = Harness::new();
            h.call("ui.set", json!({"layout": mode}));
            h.call("ui.prefs", json!({"ui_scale": scale}));
            h.app.preferences.look_at_anim = false;
            h.settle();
            let outer = crate::scenario::rect_of("axis_view:outer").unwrap();
            assert_eq!(outer.size(), vec2(116.0, 116.0));
            for (name, direction) in [("x", Vec3::X), ("-x", -Vec3::X), ("y", Vec3::Y), ("-y", -Vec3::Y), ("z", Vec3::Z), ("-z", -Vec3::Z)] {
                h.call("ui.view", json!({"view":"home"}));
                h.settle();
                let centre = outer.center() - vec2(0.0, crate::viewport::AXIS_CENTER_LIFT);
                let tips = axis_tips(&h.app.cam, centre);
                let (r, u, _) = h.app.cam.basis();
                let p = centre + vec2((direction.dot(r) * 38.0 * 0.84) as f32, -(direction.dot(u) * 38.0 * 0.84) as f32);
                assert_eq!(crate::scenario::handle_at(&format!("axis_view:{name}")), Some(p));
                for offset in [vec2(0.0, 0.0), vec2(14.9, 0.0), vec2(15.1, 0.0)] {
                    let sample = p + offset;
                    let expected = tips.iter().enumerate().rev().find(|(_, tip)| tip.2.distance(sample) <= 15.0).map(|(i, _)| i);
                    assert_eq!(axis_hit(&tips, sample), expected);
                    h.call("ui.move", json!({"x":sample.x,"y":sample.y}));
                    assert_eq!(crate::scenario::count_of("axis_view_highlights"), Some(f64::from(u8::from(expected.is_some()))));
                    if let Some(i) = expected {
                        assert!(crate::scenario::handle_at(&format!("axis_view_hover:{}", tips[i].0)).is_some());
                    }
                }
                h.call("ui.click", json!({"x":p.x,"y":p.y}));
                assert!(h.app.cam.back().dist(direction) < 1e-5, "{mode:?}, {scale}, {name}");
                let p = crate::scenario::handle_at(&format!("axis_view:{name}")).unwrap();
                h.call("ui.click", json!({"x":p.x,"y":p.y}));
                assert!(h.app.cam.back().dist(-direction) < 1e-5);
                assert_eq!(crate::scenario::rect_of("axis_view:outer"), Some(outer));
            }
        }
    }
}

#[test]
fn compass_negative_ends_are_hollow_and_positive_ends_are_chunkier() {
    for dark in [false, true] {
        let t = Tokens::themed(dark, crate::theme::ColorVision::Off);
        for (name, direction, positive) in [("x", Vec3::X, true), ("-x", -Vec3::X, false)] {
            let ctx = crate::fonts::test_context();
            let c = pos2(100.0, 100.0);
            let original = c + vec2(38.0, 0.0);
            let painted = c + (original - c) * 0.84;
            let output = draw(&ctx, |ui| {
                crate::icons::navigation::axis_tip(ui.painter(), c, name, &direction, painted, 0.5, false, t);
            });
            let circles: Vec<_> = output.shapes.iter().filter_map(|s| if let Shape::Circle(c) = &s.shape { Some(c) } else { None }).collect();
            let end = circles.iter().find(|circle| circle.center == painted).unwrap();
            if positive {
                assert_eq!(end.fill, t.axis_x);
                assert_eq!(end.radius, 11.0);
                let letters: Vec<_> = output
                    .shapes
                    .iter()
                    .filter_map(|s| match &s.shape {
                        Shape::Text(text) if text.galley.text() == "X" => Some(text),
                        _ => None,
                    })
                    .collect();
                assert_eq!(letters.len(), 1, "each axis letter is painted once");
                let letter = letters[0];
                assert_eq!(letter.galley.job.sections[0].format.font_id, crate::fonts::bold(Tokens::get().font_compass));
                assert_eq!(letter.pos.x.fract(), 0.0);
                assert_eq!(letter.pos.y.fract(), 0.0);
            } else {
                assert_eq!(end.fill, Color32::TRANSPARENT);
                assert_eq!(end.stroke.width, 2.4);
                assert_eq!(end.radius, 4.8);
                assert!(!output.shapes.iter().any(|s| matches!(&s.shape, Shape::Text(_))));
            }
            for shape in &output.shapes {
                if let Shape::LineSegment { points, stroke } = &shape.shape {
                    assert!(points[1].distance(painted) >= end.radius - 0.001, "shaft must stop at the ring");
                    assert!(stroke.width >= 2.5);
                }
            }
            assert!(painted.distance(c) + end.radius < 49.5 - 3.0);
        }
    }
}

#[test]
fn status_toast_keeps_the_messages_handle_and_opens_the_message_history() {
    for mode in [LayoutMode::Floating, LayoutMode::Docked] {
        for error in [false, true] {
            for sketch in [false, true] {
                let mut h = Harness::new();
                h.call("ui.set", json!({"layout": mode}));
                if sketch {
                    h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
                    h.app.start("sketch.line");
                }
                h.app.set_status("Viewport message", error);
                h.frames(2);
                let toast = crate::scenario::rect_of("toast").unwrap();
                let chip = crate::scenario::handle_at("status:chip").unwrap();
                assert_eq!(chip, toast.center());
                h.call("ui.click", json!({"x": chip.x, "y": chip.y}));
                h.frames(2);
                assert!(h.app.messages.open, "{mode:?}, error={error}: the toast opens Messages");
                assert!(crate::scenario::handle_at("messages:copy").is_some());
                let out = h.call("ui.messages", json!({}));
                let message = out["result"]["messages"].as_array().unwrap().last().unwrap();
                assert_eq!(message["text"], "Viewport message");
                assert_eq!(message["error"], error);
            }
        }
    }
}

#[test]
fn toast_stays_in_free_content_above_history_and_navigation() {
    for mode in [LayoutMode::Floating, LayoutMode::Docked] {
        for scale in [1.0, 2.0] {
            let mut h = Harness::new();
            h.call("ui.set", json!({"layout": mode}));
            h.call("ui.prefs", json!({"ui_scale": scale}));
            h.app.run("solid.box", json!({"length":40,"width":30,"height":20})).unwrap();
            for (width, height) in [(1400, 900), (900, 720), (700, 475), (320, 240), (1, 1)] {
                h.call("ui.resize", json!({"width":width,"height":height}));
                for dialog in [false, true] {
                    if dialog {
                        h.app.start("solid.fillet");
                    } else {
                        h.app.dialog = None;
                    }
                    h.app.status = Some(("Viewport message".into(), crate::now_ms(), false));
                    h.frames(3);
                    let layout = h.app.viewport.layout.as_ref().unwrap();
                    let content = crate::shell::content_rect(&h.app, layout.viewport);
                    let mut obstacles = crate::shell::covered_rects(&h.app);
                    obstacles.extend([layout.navigation, layout.axis_view]);
                    // The default place: the bottom of the corner column (#7893).
                    let free = corner_free(content, &obstacles);
                    if let Some(toast) = crate::scenario::rect_of("toast") {
                        assert!(toast.is_finite() && toast.is_positive());
                        assert!(content.contains_rect(toast), "{mode:?}, {scale}, {width}x{height}: {toast:?}");
                        for obstacle in obstacles {
                            assert!(!toast.intersect(obstacle).is_positive(), "{toast:?} intersects {obstacle:?}");
                        }
                        assert!((toast.right() - free.right()).abs() < 0.01, "against the column's right: {toast:?} {free:?}");
                        assert!((toast.bottom() - free.bottom()).abs() < 0.01);
                    } else {
                        assert!(free.width() < 200.0 || free.height() < 40.0, "toast unexpectedly absent: {free:?}");
                    }
                }
            }
            h.app.status = Some(("Expired".into(), crate::now_ms() - 6001.0, true));
            h.frame();
            assert!(h.app.status.is_none());
            assert!(crate::scenario::rect_of("toast").is_none());
        }
    }
}

#[test]
fn toast_card_uses_shell_tokens_wraps_and_rejects_hostile_rects() {
    let ctx = crate::fonts::test_context();
    let t = Tokens::get();
    let free = Rect::from_min_size(pos2(30.0, 30.0), vec2(200.0, 200.0));
    for (error, computing) in [(false, false), (true, false), (false, true)] {
        let output = draw(&ctx, |ui| {
            let r = paint_toast(ui, free, "A message that wraps across several short lines".into(), error, computing).unwrap();
            assert!(free.contains_rect(r));
            assert!(r.height() > 35.0);
            for bad in [
                Rect::NOTHING,
                Rect::EVERYTHING,
                Rect::from_min_size(Pos2::ZERO, vec2(1.0, 1.0)),
                Rect::from_min_size(Pos2::ZERO, vec2(41.0, 1.0)),
                Rect::from_min_max(pos2(f32::MAX * 0.75, 0.0), pos2(f32::MAX, 100.0)),
                Rect::from_min_size(Pos2::ZERO, vec2(f32::NAN, 90.0)),
            ] {
                assert!(paint_toast(ui, bad, "Message".into(), error, computing).is_none());
            }
        });
        fn card(shape: &Shape, t: Tokens) -> bool {
            match shape {
                Shape::Rect(r) => r.fill == t.panel && r.stroke.color == t.border && r.corner_radius == t.radius_md.into(),
                Shape::Vec(shapes) => shapes.iter().any(|s| card(s, t)),
                _ => false,
            }
        }
        assert!(output.shapes.iter().any(|s| card(&s.shape, t)));
        if !computing {
            assert!(
                output
                    .shapes
                    .iter()
                    .any(|s| matches!(&s.shape, Shape::Circle(c) if c.radius == 3.0 && c.fill == if error { t.error } else { t.text }))
            );
        }
    }
}

#[test]
fn sketch_previews_selection_band_and_number_focus_follow_sketch_accent() {
    for mode in [LayoutMode::Floating, LayoutMode::Docked] {
        for dark in [false, true] {
            let mut h = Harness::new();
            h.call("ui.set", json!({"layout":mode}));
            h.call("ui.prefs", json!({"theme":if dark {"dark"} else {"light"}}));
            h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
            h.call("ui.view", json!({"view":"top"}));
            h.settle();
            let proj = display_proj(&h.app);
            let t = Tokens::get();
            for cmd in ["sketch.line", "sketch.rectangle.two_point", "sketch.circle.center", "sketch.spline.fit_point"] {
                let mut tool = crate::tools::Tool::for_command(cmd).unwrap();
                tool.pts.push((Vec2::ZERO, None));
                tool.hover = Some((Vec2::new(20.0, 8.0), None));
                let output = draw(&h.ctx, |ui| crate::tools::preview(&h.app, &tool, ui.painter(), &proj));
                assert!(
                    output.shapes.iter().any(|s| match &s.shape {
                        Shape::LineSegment { stroke, .. } => stroke.color == t.sketch_accent,
                        Shape::Path(p) => matches!(p.stroke.color, egui::epaint::ColorMode::Solid(color) if color == t.sketch_accent),
                        _ => false,
                    }),
                    "{cmd}: no Sketch preview stroke"
                );
            }
            let mut extended = crate::tools::Tool::for_command("sketch.line").unwrap();
            extended.kind = crate::tools::Kind::Ext;
            extended.pts.push((Vec2::ZERO, None));
            extended.hover = Some((Vec2::new(20.0, 8.0), None));
            let output = draw(&h.ctx, |ui| crate::sketch_tools::preview(&h.app, &extended, ui.painter(), &proj));
            assert!(output.shapes.iter().any(|s| sketch_stroke(&s.shape, t.sketch_accent)));
            for sketch in [false, true] {
                for crossing in [false, true] {
                    let bx = BoxSel { rect: Rect::from_min_size(pos2(20.0, 20.0), vec2(100.0, 80.0)), crossing };
                    let output = draw(&h.ctx, |ui| paint_box_selection(ui.painter(), bx, sketch));
                    let color = if sketch {
                        t.sketch_accent
                    } else if crossing {
                        t.box_crossing
                    } else {
                        t.box_window
                    };
                    assert!(output.shapes.iter().any(|s| match &s.shape {
                        Shape::Rect(r) => r.stroke.color == color,
                        Shape::Vec(shapes) => shapes.iter().any(|s| matches!(s, Shape::LineSegment { stroke, .. } if stroke.color == color)),
                        _ => false,
                    }));
                }
            }
            h.app.ui.tab = "MODEL".into(); // A sketch remains open when another ribbon tab is selected.
            h.app.tool = crate::tools::Tool::for_command("sketch.line");
            crate::theme::TabAccent::publish(&h.app, &h.ctx);
            let output = draw(&h.ctx, |ui| {
                let r = ui.add(egui::TextEdit::singleline(&mut String::from("20")));
                r.request_focus();
                crate::theme::TabAccent::number_focus(ui, &r);
            });
            assert!(output.shapes.iter().any(|s| matches!(&s.shape, Shape::Rect(r) if r.stroke.color == t.sketch_accent)));
        }
    }
}

#[test]
fn three_dimensional_sketch_rubber_band_uses_sketch_accent() {
    for mode in [LayoutMode::Floating, LayoutMode::Docked] {
        let mut h = Harness::new();
        h.call("ui.set", json!({"layout": mode}));
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.app.run("sketch.options", json!({"sketch_3d":true})).unwrap();
        h.call("ui.view", json!({"view":"top"}));
        h.app.start("sketch.line");
        h.settle();
        h.step(&json!({"click":{"world":[0,0,0]}})).unwrap();
        h.step(&json!({"move":{"world":[20,8,0]}})).unwrap();
        let proj = display_proj(&h.app);
        let output = draw(&h.ctx, |ui| {
            assert!(crate::sketch3d::preview(&h.app, h.app.tool.as_ref().unwrap(), ui.painter(), &proj));
        });
        assert!(output.shapes.iter().any(|s| sketch_stroke(&s.shape, Tokens::get().sketch_accent)));
    }
}

#[test]
fn a_hovered_compass_end_turns_white_and_its_letter_takes_the_axis_colour() {
    for dark in [false, true] {
        let t = Tokens::themed(dark, crate::theme::ColorVision::Off);
        for (name, direction, positive) in [("x", Vec3::X, true), ("-x", -Vec3::X, false)] {
            let ctx = crate::fonts::test_context();
            let c = pos2(100.0, 100.0);
            let original = c + vec2(38.0, 0.0);
            let painted = c + (original - c) * 0.84;
            let output = draw(&ctx, |ui| {
                crate::icons::navigation::axis_tip(ui.painter(), c, name, &direction, painted, 0.5, true, t);
            });
            let circles: Vec<_> = output.shapes.iter().filter_map(|s| if let Shape::Circle(c) = &s.shape { Some(c) } else { None }).collect();
            assert!(circles.iter().any(|circle| circle.center == painted && circle.fill == t.on_accent), "{name}: the hovered end is filled white");
            if !dark {
                // White would vanish on the light compass: an axis-coloured outline keeps it visible.
                assert!(
                    circles.iter().any(|circle| circle.center == painted && circle.stroke.color == t.axis_x && circle.stroke.width >= 2.0),
                    "{name}: the hovered end is outlined in its axis colour on the light theme"
                );
            }
            let letters: Vec<_> = output.shapes.iter().filter_map(|s| if let Shape::Text(text) = &s.shape { Some(text) } else { None }).collect();
            if positive {
                assert!(
                    !letters.is_empty()
                        && letters
                            .iter()
                            .all(|text| text.override_text_color == Some(t.axis_x)
                                || text.galley.job.sections.iter().all(|s| s.format.color == t.axis_x)),
                    "the letter takes the axis colour"
                );
            } else {
                assert!(letters.is_empty());
            }
        }
    }
}

#[test]
fn compass_circle_grabs_on_hover_and_the_home_chip_points() {
    let mut h = Harness::new();
    h.call("ui.set", json!({"layout": "floating"}));
    h.settle();
    let outer = crate::scenario::rect_of("axis_view:outer").unwrap();
    let centre = outer.center() - vec2(0.0, crate::viewport::AXIS_CENTER_LIFT);
    // A point inside the circle that is clear of every axis end and of the home chip.
    let probe = centre + vec2(-36.0, 30.0);
    h.call("ui.move", json!({"x": probe.x, "y": probe.y}));
    h.settle();
    assert_eq!(h.output.cursor_icon, egui::CursorIcon::Grab, "an open hand over the circle");
    let home = crate::scenario::handle_at("axis_view:home").unwrap();
    h.call("ui.move", json!({"x": home.x, "y": home.y}));
    h.settle();
    assert_eq!(h.output.cursor_icon, egui::CursorIcon::PointingHand, "a pointer on the home chip");
    h.call("ui.move", json!({"x": centre.x - 200.0, "y": centre.y + 200.0}));
    h.settle();
    assert_ne!(h.output.cursor_icon, egui::CursorIcon::Grab, "no hand away from the compass");
}

/// At the bottom centre (the `centre` preference, the place before #7893) the status and computing
/// toasts keep the size and place they always had: the text's size plus 40 by 16, centred at the
/// bottom of the free rect.
#[test]
fn the_shared_toast_draws_old_callers_exactly_as_before() {
    let ctx = crate::fonts::test_context();
    ToastPlace::Centre.publish(&ctx);
    for width in [200.0_f32, 300.0, 500.0, 900.0] {
        let free = Rect::from_min_size(pos2(10.0, 20.0), vec2(width, 200.0));
        for (error, computing) in [(false, false), (true, false), (false, true)] {
            draw(&ctx, |ui| {
                let t = Tokens::get();
                let text = "A message that is long enough to wrap in the narrow cases, and not in the wide ones".to_string();
                let galley = ui.painter().layout(text.clone(), crate::fonts::medium(t.font_label), t.text, (free.width() - 40.0).min(440.0));
                let size = galley.size() + vec2(40.0, 16.0);
                let expected = Rect::from_center_size(pos2(free.center().x, free.bottom() - size.y * 0.5), size);
                let got = paint_toast(ui, free, text, error, computing);
                if size.x > free.width() || size.y > free.height() {
                    assert!(got.is_none());
                } else {
                    assert_eq!(got, Some(expected), "width {width} error {error} computing {computing}");
                }
            });
        }
    }
}

/// The toast's buttons stay inside its card at narrow widths, and do nothing while an issue report
/// holds CAD input.
#[test]
fn toast_buttons_wrap_inside_the_card_and_are_dead_behind_an_issue_report() {
    let labels = [("Calibrate…", ToastKind::Primary), ("Use Direct Driver", ToastKind::Secondary), ("Never show again", ToastKind::Dismiss)];
    for width in [260.0_f32, 360.0, 600.0, 1000.0] {
        for blocked in [false, true] {
            crate::issue_report::set_cad_input_blocked_for_test(blocked);
            let ctx = crate::fonts::test_context();
            let free = Rect::from_min_size(pos2(0.0, 0.0), vec2(width, 300.0));
            let run = |events: Vec<egui::Event>| {
                let mut seen = None;
                let mut output = ctx.run_ui(egui::RawInput { events, screen_rect: Some(free), focused: true, ..Default::default() }, |ui| {
                    seen = paint_toast_with(ui, free, "Your 3D mouse isn't calibrated.".into(), ToastLead::Dot(false), &labels);
                });
                output.textures_delta.clear();
                seen
            };
            run(vec![]);
            let (card, _, _) = run(vec![]).unwrap_or_else(|| panic!("width {width}: no card"));
            assert!(free.contains_rect(card), "width {width}: {card:?}");
            let mut hits = Vec::new();
            for step_x in 1..12 {
                for step_y in 1..16 {
                    let at = pos2(card.left() + card.width() * step_x as f32 / 12.0, card.top() + card.height() * step_y as f32 / 16.0);
                    run(vec![egui::Event::PointerMoved(at)]);
                    run(vec![egui::Event::PointerButton {
                        pos: at,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: Default::default(),
                    }]);
                    let clicked = run(vec![egui::Event::PointerButton {
                        pos: at,
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        modifiers: Default::default(),
                    }]);
                    if let Some((_, Some(index), _)) = clicked {
                        hits.push(index);
                    }
                }
            }
            crate::issue_report::set_cad_input_blocked_for_test(false);
            if blocked {
                assert!(hits.is_empty(), "width {width}: a button worked behind a report: {hits:?}");
            } else {
                hits.sort_unstable();
                hits.dedup();
                assert_eq!(hits, [0, 1, 2], "width {width}: every button can be clicked inside the card");
            }
        }
    }
}

/// The toast's buttons group by purpose and wrap as whole groups: the main action first, then the
/// alternative, then the dismissals; no button stands alone on a row while its group-mate fits.
#[test]
fn toast_buttons_group_by_purpose_and_wrap_as_groups() {
    use ToastKind::{Dismiss, Primary, Secondary};
    let four = [(110.0, Primary), (220.0, Secondary), (60.0, Dismiss), (140.0, Dismiss)];
    let three = [(110.0, Primary), (60.0, Dismiss), (140.0, Dismiss)];
    for card in [260.0_f32, 360.0, 460.0, 560.0, 700.0, 1000.0] {
        let row_width = (card - 40.0).min(440.0);
        for buttons in [&four[..], &three[..]] {
            let spots = toast_action_spots(buttons, row_width, 8.0, 28.0);
            let widest = buttons.iter().map(|b| b.0).fold(0.0, f32::max);
            // Nothing leaves the row (a button wider than the row is cut to it by the painter), and none overlaps.
            for (i, spot) in spots.iter().enumerate() {
                assert!(spot.left() >= 0.0 && spot.right() <= row_width.max(widest) + 0.01, "card {card}: {spot:?}");
                for other in &spots[i + 1..] {
                    assert!(!spot.intersects(*other), "card {card}: {spot:?} overlaps {other:?}");
                }
            }
            // The primary is first: top left, and no button is above it or left of it on its row.
            assert!(spots.iter().all(|s| s.top() > spots[0].top() || s.left() >= spots[0].left()), "card {card}: primary first");
            let mut rows: std::collections::BTreeMap<i32, usize> = Default::default();
            for spot in &spots {
                *rows.entry(spot.top() as i32).or_default() += 1;
            }
            // Whole groups fit the row: then no row has a button on its own, except the primary of the
            // three-button set (its group is only itself).
            let group_fits = if buttons.len() == 4 { 110.0 + 8.0 + 220.0 <= row_width } else { 140.0 + 8.0 + 60.0 <= row_width };
            if group_fits {
                let singles = rows.values().filter(|&&n| n == 1).count();
                assert!(singles <= usize::from(buttons.len() == 3), "card {card}, {} buttons: a button is left alone: {rows:?}", buttons.len());
            }
            if row_width >= 110.0 + 8.0 + 220.0 + 8.0 + 60.0 + 8.0 + 140.0 {
                assert_eq!(rows.len(), 1, "card {card}: one row when everything fits");
            }
        }
    }
}

#[test]
fn bottom_readouts_clear_floating_history_on_the_first_frame() {
    let mut h = Harness::new();
    // The corner legend remains available through the analysis HUD compatibility setting.
    h.app.preferences.viewport_analysis_status = false;
    h.app.home.open = false;
    h.app.ui.layout = LayoutMode::Floating;
    h.app.preferences.timeline_left = false;
    h.app.preferences.show_frame_rate = true;
    for command in ["inspect.draft", "inspect.accessibility", "inspect.curvature_map"] {
        h.app.session.analysis = None;
        h.app.run(command, json!({})).unwrap();
        for shown in [false, true, false, true] {
            h.app.ui.show_timeline = shown;
            h.frame();
            let viewport = h.app.viewport.layout.as_ref().unwrap().viewport;
            let history = crate::scenario::rect_of("history:card");
            assert_eq!(history.is_some(), shown);
            for name in ["analysis:legend", "viewport:frame_rate"] {
                let readout = crate::scenario::rect_of(name).unwrap();
                assert!(viewport.contains_rect(readout), "{name}: {readout:?}");
                if let Some(history) = history {
                    assert!(!readout.intersects(history), "{name}: {readout:?} overlaps {history:?}");
                } else {
                    assert!((readout.bottom() - (viewport.bottom() - 12.0)).abs() < 0.01);
                }
            }
        }
    }
    h.app.session.analysis = None;
    h.app.run("inspect.draft", json!({})).unwrap();
    // Start with a measured floating card, then switch without a settling frame.
    h.app.ui.layout = LayoutMode::Floating;
    h.app.ui.show_timeline = true;
    h.frame();
    h.app.ui.layout = LayoutMode::Docked;
    h.frame();
    let viewport = h.app.viewport.layout.as_ref().unwrap().viewport;
    let history = crate::scenario::rect_of("history:strip").unwrap();
    for name in ["analysis:legend", "viewport:frame_rate"] {
        let readout = crate::scenario::rect_of(name).unwrap();
        assert!(!readout.intersects(history));
        assert!((readout.bottom() - (viewport.bottom() - 12.0)).abs() < 0.01);
    }
}

#[test]
fn continuity_legend_clears_floating_history() {
    let mut h = Harness::new();
    h.app.ui.layout = LayoutMode::Floating;
    h.app.preferences.timeline_left = false;
    h.app.ui.show_timeline = true;
    h.app.run("solid.box", json!({"length":20,"width":20,"height":20})).unwrap();
    h.app.start("inspect.continuity");
    h.frame();
    let legend = crate::scenario::rect_of("continuity_legend").unwrap();
    let history = crate::scenario::rect_of("history:card").unwrap();
    assert!(!legend.intersects(history), "{legend:?} overlaps {history:?}");
}

#[test]
fn left_history_sidebar_leaves_bottom_readouts_at_the_bottom() {
    let mut h = Harness::new();
    // The corner legend remains available through the analysis HUD compatibility setting.
    h.app.preferences.viewport_analysis_status = false;
    h.app.home.open = false;
    h.app.ui.layout = LayoutMode::Floating;
    h.app.preferences.timeline_left = true;
    h.app.preferences.show_frame_rate = true;
    h.app.ui.show_timeline = true;
    h.app.run("inspect.draft", json!({})).unwrap();
    h.frame();
    let viewport = h.app.viewport.layout.as_ref().unwrap().viewport;
    let history = crate::scenario::rect_of("history:vertical").expect("left History");
    for name in ["analysis:legend", "viewport:frame_rate"] {
        let readout = crate::scenario::rect_of(name).unwrap();
        assert!((readout.bottom() - (viewport.bottom() - 12.0)).abs() < 0.01, "{name}: {readout:?} in {viewport:?}");
        assert!(!readout.intersects(history), "{name}: {readout:?} under History {history:?}");
    }
    for command in ["inspect.accessibility", "inspect.curvature_map"] {
        h.app.run(command, json!({})).unwrap();
        h.frame();
        let legend = crate::scenario::rect_of("analysis:legend").unwrap();
        assert!(!legend.intersects(history), "{command}: {legend:?} under History {history:?}");
        assert!(legend.left() >= history.right() - 0.5, "{command}: legend stays right of History");
    }
}

/// The frame-rate readout and its details sit just left of a pane over the view's bottom-right
/// corner (the Assistant), follow it as it opens, changes width and closes, and stay in the
/// visible view; with the pane on the left they keep the corner (#6755).
#[test]
fn frame_rate_readout_stays_beside_a_right_pane() {
    use crate::scenario::{handle_at, rect_of};
    for mode in [LayoutMode::Floating, LayoutMode::Docked] {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.call("ui.set", json!({"layout": mode}));
        h.app.preferences.show_frame_rate = true;
        h.app.preferences.frame_rate_details = true;
        h.app.preferences.agent_chat_side = "right".into();
        h.frames(3);
        let in_corner = |h: &Harness| {
            let viewport = h.app.viewport.layout.as_ref().unwrap().viewport;
            let readout = rect_of("viewport:frame_rate").expect("the readout is drawn");
            assert!((readout.right() - (viewport.right() - 12.0)).abs() < 0.01, "{mode:?}: {readout:?} in {viewport:?}");
        };
        in_corner(&h);
        // The Assistant's narrowest, a middle and its widest width.
        for width in [280.0, 480.0, 760.0] {
            h.call("ui.agent_chat", json!({"open": true, "picker": false, "side": "right", "width": width}));
            h.frames(3);
            let viewport = h.app.viewport.layout.as_ref().unwrap().viewport;
            let pane = rect_of("agent_chat:panel").expect("the Assistant is drawn");
            let readout = rect_of("viewport:frame_rate").expect("the readout is drawn");
            assert!(viewport.contains_rect(readout), "{mode:?}, {width}: {readout:?} outside {viewport:?}");
            assert!(readout.right() <= pane.left() - 12.0 + 0.01, "{mode:?}, {width}: {readout:?} under {pane:?}");
            if mode == LayoutMode::Floating {
                assert!((readout.right() - (pane.left() - 12.0)).abs() < 0.01, "{width}: {readout:?} not just left of {pane:?}");
            }
            for covered in crate::shell::covered_rects(&h.app) {
                assert!(!readout.intersects(covered), "{mode:?}, {width}: {readout:?} under {covered:?}");
            }
            let details = handle_at("viewport:frame_details").expect("the details are drawn");
            assert!(viewport.contains(details) && details.x < pane.left(), "{mode:?}, {width}: details at {details:?}, pane {pane:?}");
        }
        h.call("ui.agent_chat", json!({"open": false}));
        h.frames(3);
        assert!(rect_of("agent_chat:panel").is_none());
        in_corner(&h);
        // On the left the pane leaves the corner free.
        h.call("ui.agent_chat", json!({"open": true, "side": "left"}));
        h.frames(3);
        assert!(rect_of("agent_chat:panel").is_some());
        in_corner(&h);
    }
}

/// The View angles readout (#6032) sits under the orientation gizmo only while View ▸ View angles
/// is on, follows the view live with its projection, and its card turns the view to typed angles
/// through `view.apply`, without touching the design or its history.
#[test]
fn view_angles_readout_shows_only_when_on_and_applies_typed_angles() {
    use solidcraft_engine::render::ViewAngles;
    for style in ["axes", "cube"] {
        let mut h = Harness::new();
        h.call("ui.prefs", json!({"navigator_style": style}));
        h.app.preferences.look_at_anim = false;
        h.settle();
        let design = h.app.session.doc.to_json();
        assert!(!h.app.preferences.show_view_angles, "off by default");
        assert!(crate::scenario::rect_of("view_angles:readout").is_none());
        // View ▸ View angles runs view.angle_readout.
        let view = crate::scenario::handle_at("nav:view").unwrap();
        h.call("ui.click", json!({"x": view.x, "y": view.y}));
        let check = crate::scenario::handle_at("view:view_angles").unwrap();
        h.call("ui.click", json!({"x": check.x, "y": check.y}));
        assert!(h.app.preferences.show_view_angles);
        h.call("ui.click", json!({"x": view.x, "y": view.y}));
        h.call("ui.view", json!({"view": "front"}));
        h.settle();
        let readout = crate::scenario::rect_of("view_angles:readout").expect("the readout shows once it is on");
        let axis = crate::scenario::rect_of("axis_view:outer").unwrap();
        assert!(
            readout.top() >= axis.bottom() && readout.right() <= axis.right() && readout.right() > axis.center().x,
            "{style}: {readout:?} under {axis:?}"
        );
        let lines = |h: &Harness| crate::view_angles::lines(&h.app);
        assert_eq!(lines(&h), ("Pitch 0.0° · Yaw 0.0° · Roll 0.0°".to_string(), "Ortho".to_string()));
        h.call("ui.view", json!({"view": "right"}));
        h.app.ui.perspective = true;
        h.settle();
        assert_eq!(lines(&h), ("Pitch 0.0° · Yaw -90.0° · Roll 0.0°".to_string(), "Persp".to_string()));
        h.call("ui.view", json!({"view": "top"}));
        h.settle();
        assert_eq!(lines(&h).0, "Pitch 90.0° · Yaw 0.0° · Roll 0.0°");
        // A click on the readout opens the card with the current angles; Apply turns the view.
        let camera = h.app.cam;
        h.call("ui.click", json!({"x": readout.center().x, "y": readout.center().y}));
        assert_eq!(h.app.cam, camera, "the click opens the card, it does not move the view");
        assert!(crate::scenario::rect_of("view_angles:card").is_some());
        let draft = h.ctx.data(|d| d.get_temp::<ViewAngles>(crate::view_angles::draft_id())).expect("the card holds the typed angles");
        assert!((draft.pitch - 90.0).abs() < 1e-3 && draft.yaw == 0.0 && draft.roll == 0.0, "{draft:?}");
        h.ctx.data_mut(|d| d.insert_temp(crate::view_angles::draft_id(), ViewAngles { yaw: 30.0, pitch: 15.0, roll: -10.0 }));
        h.frame();
        let apply = crate::scenario::handle_at("dialog:button:Apply").unwrap();
        h.call("ui.click", json!({"x": apply.x, "y": apply.y}));
        h.settle();
        let a = h.app.cam.view_angles();
        assert!((a.yaw - 30.0).abs() < 1e-9 && (a.pitch - 15.0).abs() < 1e-9 && (a.roll + 10.0).abs() < 1e-9, "{a:?}");
        assert_eq!((h.app.cam.target, h.app.cam.distance), (camera.target, camera.distance), "only the orientation changes");
        assert_eq!(lines(&h).0, "Pitch 15.0° · Yaw 30.0° · Roll -10.0°");
        // A click elsewhere closes the card; the command turns the readout off again.
        let centre = h.app.viewport.rect.unwrap().center();
        h.call("ui.click", json!({"x": centre.x, "y": centre.y}));
        assert!(crate::scenario::rect_of("view_angles:card").is_none());
        h.call("engine.execute", json!({"command": "view.angle_readout", "params": {"enabled": false}}));
        h.settle();
        assert!(!h.app.preferences.show_view_angles);
        assert!(crate::scenario::rect_of("view_angles:readout").is_none());
        assert_eq!(h.app.session.doc.to_json(), design);
        assert!(h.app.session.undo.is_empty());
    }
}

/// In the corner (#7893) the buttons sit as in the dialogs: right-aligned, dismissals first and the
/// main action right-most; when they wrap, the row with the main action is the bottom one.
#[test]
fn corner_toast_buttons_follow_the_dialogs_and_keep_the_main_action_bottom_right() {
    use ToastKind::{Dismiss, Primary, Secondary};
    let row_width = TOAST_WIDTH - 40.0;
    // Three short ones fit one row: Close, Don't ask again, Stand it up.
    let three = [(90.0, Primary), (110.0, Secondary), (50.0, Dismiss)];
    let spots = toast_action_spots_trailing(&three, row_width, 8.0, 28.0);
    assert!(spots.iter().all(|s| s.top() == 0.0), "one row: {spots:?}");
    assert!((spots[0].right() - row_width).abs() < 0.01, "the main action is right-most: {spots:?}");
    assert!(spots[2].right() < spots[1].left() && spots[1].right() < spots[0].left(), "dismiss, alternative, main: {spots:?}");
    // The 3D mouse offer's four wrap: the main action and its alternative below, the dismissals above.
    let four = [(90.0, Primary), (190.0, Secondary), (50.0, Dismiss), (120.0, Dismiss)];
    let spots = toast_action_spots_trailing(&four, row_width, 8.0, 28.0);
    let bottom = spots.iter().map(|s| s.top()).fold(0.0, f32::max);
    assert!(bottom > 0.0, "wrapped: {spots:?}");
    assert_eq!(spots[0].top(), bottom, "the main action is on the bottom row");
    assert!((spots[0].right() - row_width).abs() < 0.01, "and right-most");
    for (i, spot) in spots.iter().enumerate() {
        assert!(spot.left() >= -0.01 && spot.right() <= row_width + 0.01, "inside the row: {spot:?}");
        for other in &spots[i + 1..] {
            assert!(!spot.intersects(*other), "{spot:?} overlaps {other:?}");
        }
    }
    // Hostile widths never panic and give one spot per button.
    for bad in [f32::NAN, f32::INFINITY, -5.0, 0.0] {
        assert_eq!(toast_action_spots_trailing(&four, bad, 8.0, 28.0).len(), 4);
    }
}

/// In the corner every toast is `TOAST_WIDTH` wide, whatever its text, and sits against the right of
/// `free`; at the centre (the old place, a preference) each is as wide as its text, centred.
#[test]
fn corner_toasts_share_one_width_against_the_right_edge() {
    let ctx = crate::fonts::test_context();
    let free = Rect::from_min_size(pos2(0.0, 0.0), vec2(1200.0, 700.0));
    for place in [ToastPlace::Corner, ToastPlace::Centre] {
        place.publish(&ctx);
        let mut rects = Vec::new();
        draw(&ctx, |ui| {
            for text in ["Open done", "Calibrating your 3D mouse fixes swapped or reversed axes and sets its speed and feel."] {
                rects.push(paint_toast(ui, free, text.into(), false, false).unwrap());
            }
        });
        match place {
            ToastPlace::Corner => {
                for r in &rects {
                    assert!((r.width() - TOAST_WIDTH).abs() < 0.5, "{r:?}");
                    assert!((r.right() - free.right()).abs() < 0.5, "against the right edge: {r:?}");
                }
            }
            ToastPlace::Centre => {
                assert!(rects[0].width() < rects[1].width(), "as wide as their text: {rects:?}");
                for r in &rects {
                    assert!((r.center().x - free.center().x).abs() < 0.5, "centred: {r:?}");
                }
            }
        }
    }
    ToastPlace::Corner.publish(&ctx);
}

/// A pane at the right edge (an edge-docked dialog, the Assistant) takes the corner column's
/// width with it: the stack moves, at full `TOAST_WIDTH`, to just left of the pane instead of
/// being squeezed into the strip beside it (#8804, as the frame-rate readout does, #6755).
#[test]
fn corner_toasts_move_left_of_a_pane_at_the_right_edge_at_full_width() {
    let t = Tokens::get();
    let view = Rect::from_min_size(Pos2::ZERO, vec2(1600.0, 1000.0));
    let axis = Rect::from_min_max(pos2(1600.0 - 8.0 - 110.0, 8.0), pos2(1592.0, 118.0));
    let alone = corner_free(view, &[axis]);
    assert!((alone.right() - (view.right() - t.space_8)).abs() < 0.01, "no pane keeps the corner: {alone:?}");
    for width in [230.0, 280.0, 480.0, 760.0] {
        let pane = Rect::from_min_max(pos2(1600.0 - 8.0 - width, 40.0), pos2(1592.0, 990.0));
        let free = corner_free(view, &[axis, pane]);
        assert!(free.width() >= TOAST_WIDTH - 0.5, "{width}: squeezed to {free:?}");
        assert!(!free.intersects(pane), "{width}: {free:?} over {pane:?}");
        assert!(free.right() <= pane.left() - t.space_8 + 0.01, "{width}: {free:?} not left of {pane:?}");
        assert!(view.contains_rect(free), "{width}: {free:?} outside the view");
    }
    // Nothing is wide enough: a tall place left of the pane beats a full-width strip a line high.
    let small = Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 1000.0));
    let axis = Rect::from_min_max(pos2(1000.0 - 8.0 - 110.0, 8.0), pos2(992.0, 118.0));
    let pane = Rect::from_min_max(pos2(1000.0 - 8.0 - 760.0, 40.0), pos2(992.0, 990.0));
    let free = corner_free(small, &[axis, pane]);
    assert!(free.width() > 40.0 && free.height() >= 8.0 * t.space_8, "a strip too flat to show a toast: {free:?}");
    assert!(!free.intersects(pane), "{free:?} over {pane:?}");
    // Hostile input never panics or loops, and gives a finite rect or nothing.
    let nan = Rect::from_min_max(pos2(f32::NAN, 0.0), pos2(10.0, f32::NAN));
    let inf = Rect::from_min_max(pos2(f32::NEG_INFINITY, f32::NEG_INFINITY), pos2(f32::INFINITY, f32::INFINITY));
    let many: Vec<Rect> = (0..40).map(|i| Rect::from_min_max(pos2(i as f32 * 30.0, 0.0), pos2(i as f32 * 30.0 + 20.0, 1000.0))).collect();
    let cases: [(Rect, Vec<Rect>); 5] = [
        (view, vec![nan, inf]),
        (view, vec![view]),
        (Rect::from_min_size(Pos2::ZERO, vec2(1.0, 1.0)), vec![axis]),
        (view, many),
        (Rect::NOTHING, vec![axis]),
    ];
    for (v, obstacles) in &cases {
        let free = corner_free(*v, obstacles);
        assert!(!free.is_positive() || (free.is_finite() && v.contains_rect(free)), "{free:?} in {v:?}");
    }
}

/// With a real dialog docked at the right edge, a long status toast stays `TOAST_WIDTH` wide left
/// of it, clear of everything covered, and goes back to the corner when the dialog closes (#8804).
#[test]
fn corner_toast_keeps_its_width_beside_a_docked_dialog() {
    use crate::scenario::rect_of;
    let t = Tokens::get();
    for mode in [LayoutMode::Floating, LayoutMode::Docked] {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.call("ui.set", json!({"layout": mode}));
        h.call("ui.resize", json!({"width":1600,"height":1000}));
        let text = "Calibrating your 3D mouse fixes swapped or reversed axes and sets its speed and feel.";
        h.app.start("parts.insert");
        h.app.status = Some((text.into(), crate::now_ms(), true));
        h.frames(3);
        let dialog = h.app.viewport.dialog_rect.expect("the dialog is open");
        let layout = h.app.viewport.layout.as_ref().unwrap();
        let content = crate::shell::content_rect(&h.app, layout.viewport);
        let toast = rect_of("toast").expect("the toast is drawn beside the dialog");
        assert!((toast.width() - TOAST_WIDTH).abs() < 0.5, "{mode:?}: {toast:?} beside {dialog:?}");
        assert!(!toast.intersects(dialog), "{mode:?}: {toast:?} over {dialog:?}");
        assert!(content.contains_rect(toast), "{mode:?}: {toast:?} outside {content:?}");
        for covered in crate::shell::covered_rects(&h.app) {
            assert!(!toast.intersects(covered), "{mode:?}: {toast:?} under {covered:?}");
        }
        h.app.dialog = None;
        h.app.status = Some((text.into(), crate::now_ms(), true));
        h.frames(3);
        let layout = h.app.viewport.layout.as_ref().unwrap();
        let content = crate::shell::content_rect(&h.app, layout.viewport);
        let toast = rect_of("toast").expect("the toast is drawn");
        assert!((toast.right() - (content.right() - t.space_8)).abs() < 0.5, "{mode:?}: back in the corner: {toast:?} in {content:?}");
        assert!((toast.width() - TOAST_WIDTH).abs() < 0.5, "{mode:?}: {toast:?}");
    }
}

/// A dashed line reaching far off screen is cut to the view before it is dashed: epaint makes a
/// shape per dash and steps along in f32, so one a trillion points long made billions of them,
/// or never finished. A line in view is dashed exactly as before, and a cut one keeps its
/// dashes in phase with its start.
#[test]
fn a_dashed_line_far_off_screen_is_cut_to_the_view() {
    let view = Rect::from_min_size(Pos2::ZERO, vec2(1600.0, 1000.0));
    let stroke = Stroke::new(1.0, Color32::WHITE);
    let near = [pos2(100.0, 100.0), pos2(700.0, 400.0), pos2(900.0, 100.0)];
    assert_eq!(dashed_line(&near, view, stroke, 5.0, 3.0), Shape::dashed_line(&near, stroke, 5.0, 3.0));
    let far = dashed_line(&[pos2(100.0, 500.0), pos2(1e12, 500.0)], view, stroke, 5.0, 3.0);
    assert!(!far.is_empty() && far.len() < 250, "{} dashes", far.len());
    for s in &far {
        let Shape::LineSegment { points, .. } = s else { panic!("{s:?}") };
        let periods = (points[0].x - 100.0) / 8.0;
        assert!((periods - periods.round()).abs() < 1e-3, "a dash starts at {points:?}, off the line's pattern");
    }
    let across = dashed_line(&[pos2(-1e12, 500.0), pos2(1e12, 500.0)], view, stroke, 5.0, 3.0);
    assert!(across.len() < 250, "{} dashes", across.len());
    assert!(across.iter().any(|s| matches!(s, Shape::LineSegment { points, .. } if (700.0..900.0).contains(&points[0].x))), "the line shows in view");
    for missing in [[pos2(f32::NAN, 0.0), pos2(10.0, 10.0)], [pos2(f32::INFINITY, 0.0), pos2(10.0, 10.0)], [pos2(-1e9, -1e9), pos2(-1e9, 1e9)]] {
        assert!(dashed_line(&missing, view, stroke, 5.0, 3.0).is_empty(), "{missing:?}");
    }
}

/// Extrude with a distance of 1e9 mm drew the taper handle's dashed line a billion millimetres
/// long into the viewport: the frame never finished and filled the memory. It now draws at once.
#[test]
fn a_huge_extrude_distance_draws_its_handles_at_once() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length": 40, "width": 30, "height": 20})).unwrap();
    h.settle();
    h.step(&json!({"start": "solid.extrude"})).unwrap();
    h.step(&json!({"click": {"world": [20, 15, 20]}})).unwrap();
    let started = std::time::Instant::now();
    h.step(&json!({"text": "1e9"})).unwrap();
    h.frames(4);
    assert!(crate::scenario::handle_at("taper").is_some(), "the taper handle is drawn");
    assert!(started.elapsed().as_secs_f64() < 10.0, "{:?}", started.elapsed());
}

fn point_overlays(app: &SolveApp) -> Vec<Shape> {
    let ctx = egui::Context::default();
    let proj = display_proj(app);
    let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(proj.rect), ..Default::default() }, |ui| {
        overlays(app, &ui.ctx().layer_painter(egui::LayerId::background()), &proj);
    });
    output.textures_delta.clear();
    output.shapes.into_iter().map(|shape| shape.shape).collect()
}

fn point_selection_rings(shapes: &[Shape]) -> Vec<Pos2> {
    shapes
        .iter()
        .filter_map(|shape| match shape {
            Shape::Circle(circle)
                if circle.radius == 6.0 && circle.stroke == Stroke::new(2.0, Tokens::get().sel_edge) && circle.fill == Color32::TRANSPARENT =>
            {
                Some(circle.center)
            }
            _ => None,
        })
        .collect()
}

#[test]
fn selected_sketch_points_stay_marked_after_hover_and_in_dialogs() {
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.line", json!({"points":[[10,10],[30,10]]})).unwrap();
    h.call("ui.view", json!({"view":"top"}));
    h.settle();
    let plain = point_overlays(&h.app);
    assert!(point_selection_rings(&plain).is_empty());
    h.step(&json!({"click":{"sketch":[10,10]}})).unwrap();
    h.step(&json!({"click":{"sketch":[30,10]},"shift":true})).unwrap();
    h.step(&json!({"move":[5,5]})).unwrap();
    assert!(h.app.viewport.hover.is_none());
    assert_eq!(h.app.session.selection.iter().filter(|sel| matches!(sel, Sel::SketchPoint { .. })).count(), 2);
    let marked = point_overlays(&h.app);
    let proj = display_proj(&h.app);
    let expected = [Vec3::new(10.0, 10.0, 0.0), Vec3::new(30.0, 10.0, 0.0)].map(|point| proj.to_screen(point).unwrap());
    let rings = point_selection_rings(&marked);
    assert_eq!(rings.len(), 2, "selection must remain visible after hover leaves");
    assert!(expected.iter().all(|point| rings.iter().any(|ring| ring.distance(*point) < 0.01)));
    // The markers that describe the points' constraint state keep their original fill.
    let fills = |shapes: &[Shape]| {
        shapes.iter().filter_map(|shape| if let Shape::Rect(rect) = shape { Some((rect.rect, rect.fill)) } else { None }).collect::<Vec<_>>()
    };
    assert_eq!(fills(&plain), fills(&marked));
    h.app.start("sketch.mirror");
    assert!(h.app.session.selection.is_empty(), "preselection transfers to the dialog");
    assert_eq!(point_selection_rings(&point_overlays(&h.app)), rings, "dialog inputs stay highlighted");
    h.call("ui.key", json!({"key":"Escape"}));
    h.app.run("select.clear", json!({})).unwrap();
    assert!(point_selection_rings(&point_overlays(&h.app)).is_empty());
}

#[test]
fn selected_sketch_point_aliases_respect_owner_and_visibility() {
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.line", json!({"points":[[10,10],[30,10]]})).unwrap();
    h.app.run("sketch.point", json!({"point":[20,20],"id":"custom:point"})).unwrap();
    h.call("ui.view", json!({"view":"top"}));
    h.settle();
    let sid = h.app.session.active_sketch.unwrap();
    let origin = h.app.session.world_state().sketch(sid).unwrap().sketch.points[0].id.clone();
    let ids = [origin, "custom:point".into(), "l1.end".into(), format!("{sid}:l1.end"), format!("{}:l1.start", sid + 1)];
    h.app.run("select.set", json!({"items":ids.iter().map(|id| json!({"type":"sketch_point","id":id})).collect::<Vec<_>>()})).unwrap();
    let rings = point_selection_rings(&point_overlays(&h.app));
    let proj = display_proj(&h.app);
    let expected = [Vec3::ZERO, Vec3::new(20.0, 20.0, 0.0), Vec3::new(30.0, 10.0, 0.0)].map(|point| proj.to_screen(point).unwrap());
    assert_eq!(rings.len(), 3, "aliases deduplicate, colon-containing local IDs survive, foreign owners are ignored");
    assert!(expected.iter().all(|point| rings.iter().any(|ring| ring.distance(*point) < 0.01)));
    h.app.run("sketch.options", json!({"show_points":false})).unwrap();
    assert_eq!(
        point_selection_rings(&point_overlays(&h.app)),
        vec![expected[0]],
        "selection does not reveal hidden points; the origin stays visible"
    );
}

#[test]
fn live_analysis_status_uses_session_settings_and_declared_clear_without_history() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10})).unwrap();
    h.app.session.mark_saved();
    let before = (h.app.session.doc.to_json(), h.app.session.undo.len());
    for (command, params, expected, key) in [
        ("inspect.zebra", json!({"stripes":14.5}), "14.5 stripes", false),
        ("inspect.draft", json!({"pull":[0,0,1],"angle":7.5}), "Z · 7.5°", true),
        ("inspect.environment_map", json!({}), "", false),
        ("inspect.accessibility", json!({"direction":[0,0,1]}), "from Z", true),
        ("inspect.curvature_map", json!({"radius":12}), "R 12.00 mm", true),
        ("inspect.continuity", json!({"edges":[[5,0,0]],"samples":5}), "1 edges", false),
    ] {
        h.app.run(command, params).unwrap();
        let effect = h.app.session.status_items().remove(0);
        let item = crate::viewport_status::presentation(&h.app, &effect).unwrap();
        assert_eq!(item.setting, expected);
        assert_eq!(item.has_key, key);
        assert_eq!(item.name, crate::toolbar::persistent_button_name(&h.app, solidcraft_engine::find_command(command).unwrap()));
        h.frames(4);
        assert!(crate::scenario::rect_of(&format!("viewport_status:{}:item", item.id)).is_some());
        if key {
            assert!(crate::scenario::rect_of("analysis:legend").is_none());
            h.step(&json!({"click":{"handle":format!("viewport_status:{}:key",item.id)}})).unwrap();
            h.frames(3);
            let pill = crate::scenario::rect_of(&format!("viewport_status:{}:item", item.id)).unwrap();
            let card = crate::scenario::rect_of("viewport_status:card").unwrap();
            assert!(card.top() > pill.bottom());
            assert!((card.left() - pill.left()).abs() < 1.0);
            h.step(&json!({"key":"Escape"})).unwrap();
            assert!(h.app.viewport_status.open_key.is_none());
            assert_eq!(h.app.session.status_items()[0].command, command);
        }
        h.step(&json!({"click":{"handle":format!("viewport_status:{}:close",item.id)}})).unwrap();
        assert!(h.app.session.status_items().is_empty());
        assert_eq!((h.app.session.doc.to_json(), h.app.session.undo.len()), before);
        assert!(!h.app.session.is_dirty());
    }
}

#[test]
fn analysis_status_preference_restores_corner_legend_and_old_escape() {
    let mut h = Harness::new();
    h.app.run("inspect.draft", json!({})).unwrap();
    h.call("ui.prefs", json!({"viewport_analysis_status":false}));
    h.frames(4);
    assert!(crate::scenario::rect_of("analysis:legend").is_some());
    assert!(crate::scenario::rect_of("viewport_status:row").is_none());
    h.step(&json!({"key":"Escape"})).unwrap();
    assert!(h.app.session.analysis.is_some());
    assert!(h.app.preferences.persistent_ribbon_pressed);
    assert!(h.app.session.single_surface_analysis());
    h.call("ui.prefs", json!({"viewport_analysis_status":true}));
    h.frames(4);
    assert!(crate::scenario::rect_of("analysis:legend").is_none());
    assert!(crate::scenario::rect_of("viewport_status:row").is_some());
    h.step(&json!({"key":"Escape"})).unwrap();
    assert!(h.app.session.analysis.is_none());
}

#[test]
fn analysis_status_formats_units_and_truthful_non_axis_directions() {
    let mut h = Harness::new();
    h.app.run("inspect.draft", json!({"pull":[1,1,0],"angle":2})).unwrap();
    let effect = h.app.session.status_items().remove(0);
    assert_eq!(crate::viewport_status::presentation(&h.app, &effect).unwrap().setting, "(0.71, 0.71, 0.00) · 2°");
    h.app.run("inspect.accessibility", json!({"direction":[0,0,-1]})).unwrap();
    let effect = h.app.session.status_items().remove(0);
    assert_eq!(crate::viewport_status::presentation(&h.app, &effect).unwrap().setting, "from −Z");
    h.app.run("inspect.curvature_map", json!({"radius":25.4})).unwrap();
    let effect = h.app.session.status_items().remove(0);
    for (units, expected) in [("mm", "R 25.40 mm"), ("in", "R 1.000 in")] {
        h.app.session.doc_mut().units = units.into();
        assert_eq!(crate::viewport_status::presentation(&h.app, &effect).unwrap().setting, expected);
    }
}

#[test]
fn analysis_status_escape_ends_live_tool_before_effect_and_never_hidden_section() {
    let mut h = Harness::new();
    h.app.preferences.viewport_section_status = false;
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10})).unwrap();
    h.app.start("inspect.continuity");
    assert!(h.app.tool.is_some());
    assert!(!h.app.session.continuity.is_empty());
    h.frames(3);
    h.step(&json!({"key":"Escape"})).unwrap();
    assert!(h.app.tool.is_none());
    assert!(!h.app.session.continuity.is_empty(), "first Escape only puts down the live tool");
    h.step(&json!({"key":"Escape"})).unwrap();
    assert!(h.app.session.continuity.is_empty());
    h.app.run("inspect.zebra", json!({})).unwrap();
    h.app.run("inspect.section", json!({"plane":"XY"})).unwrap();
    h.app.run("inspect.draft", json!({})).unwrap();
    h.app.run("inspect.section", json!({"plane":"XZ"})).unwrap();
    h.frames(3);
    h.step(&json!({"key":"Escape"})).unwrap();
    assert!(h.app.session.analysis.is_none());
    assert!(h.app.session.section.is_some(), "a later-issue effect with no presented item is ineligible");
}

#[test]
fn analysis_status_escape_yields_to_search_even_without_text_field_focus() {
    let mut h = Harness::new();
    h.app.run("inspect.draft", json!({})).unwrap();
    h.frames(3);
    let effect = h.app.session.status_items().remove(0);
    let item = crate::viewport_status::presentation(&h.app, &effect).unwrap();
    h.step(&json!({"click":{"handle":format!("viewport_status:{}:key",item.id)}})).unwrap();
    assert!(h.app.viewport_status.open_key.is_some());
    h.app.start("ui.palette");
    h.frames(3);
    h.ctx.memory_mut(|memory| {
        if let Some(id) = memory.focused() {
            memory.surrender_focus(id);
        }
    });
    h.step(&json!({"key":"Escape"})).unwrap();
    assert!(!h.app.ui.palette_open, "Search commands owns the first Escape");
    assert!(h.app.viewport_status.open_key.is_some());
    assert!(h.app.session.analysis.is_some());
    h.step(&json!({"key":"Escape"})).unwrap();
    assert!(h.app.viewport_status.open_key.is_none());
    assert!(h.app.session.analysis.is_some(), "the following Escape closes only the analysis key");
}

#[test]
fn analysis_status_ignores_invalid_layout_bounds_without_ending_the_effect() {
    let mut h = Harness::new();
    h.app.run("inspect.draft", json!({})).unwrap();
    let before = (h.app.session.status_items(), h.app.session.revision);
    for bounds in [Rect::NOTHING, Rect::from_min_max(pos2(165.0, 165.0), pos2(1.0, 1.0)), Rect::from_min_max(pos2(f32::NAN, 0.0), pos2(10.0, 10.0))] {
        crate::viewport_status::show(&mut h.app, &h.ctx, bounds);
        assert_eq!((h.app.session.status_items(), h.app.session.revision), before);
    }
}

#[test]
fn section_status_formats_actual_geometry_in_design_units_without_flip_sign_changes() {
    let mut h = Harness::new();
    for (params, metric, imperial) in [
        (json!({"plane":"XY","at":25.4,"hatch":false}), "Z 25.40 mm", "Z 1.000 in"),
        (json!({"plane":"XZ","at":-25.4,"flip":true}), "Y -25.40 mm · hatch", "Y -1.000 in · hatch"),
        (json!({"plane":{"origin":[-25.4,8,2],"normal":[-3,0,0]},"hatch":false}), "X -25.40 mm", "X -1.000 in"),
        (json!({"plane":{"origin":[0,0,25.4],"normal":[1e-10,0,-2]},"hatch":false}), "Z 25.40 mm", "Z 1.000 in"),
        (json!({"plane":{"origin":[-12,-9,4],"normal":[3,4,0]},"hatch":false}), "tilted · -14.40 mm", "tilted · -0.5669 in"),
        (json!({"plane":{"origin":[-12,-9,4],"normal":[3,4,0]},"flip":true}), "tilted · -14.40 mm · hatch", "tilted · -0.5669 in · hatch"),
    ] {
        h.app.session.doc_mut().units = "mm".into();
        h.app.run("inspect.section", params).unwrap();
        let effect = h.app.session.status_items().into_iter().find(|e| e.command == "inspect.section").unwrap();
        for (units, expected) in [("mm", metric), ("in", imperial)] {
            h.app.session.doc_mut().units = units.into();
            let item = crate::viewport_status::presentation(&h.app, &effect).unwrap();
            assert_eq!(item.setting, expected);
            assert!(item.editable && !item.has_key);
            assert_eq!(item.name, crate::toolbar::persistent_button_name(&h.app, solidcraft_engine::find_command("inspect.section").unwrap()));
        }
    }
}

#[test]
fn section_status_and_analysis_keep_slots_clear_independently_and_filter_escape_by_preferences() {
    let mut h = Harness::new();
    h.app.run("inspect.zebra", json!({})).unwrap();
    h.app.run("inspect.section", json!({"plane":"XY","at":4})).unwrap();
    h.app.run("inspect.draft", json!({})).unwrap();
    let before = h.app.session.status_items();
    assert_eq!(before.iter().map(|e| e.command.as_str()).collect::<Vec<_>>(), ["inspect.draft", "inspect.section"]);
    assert!(crate::viewport_status::escape(&mut h.app, false));
    assert!(h.app.session.analysis.is_none() && h.app.session.section.is_some());
    assert!(crate::viewport_status::escape(&mut h.app, true));
    assert!(h.app.session.section.is_some(), "repeat cannot cascade");
    h.app.run("inspect.zebra", json!({})).unwrap();
    h.app.run("inspect.section", json!({"plane":"YZ","at":2})).unwrap();
    h.app.preferences.viewport_section_status = false;
    assert!(crate::viewport_status::escape(&mut h.app, false));
    assert!(h.app.session.analysis.is_none() && h.app.session.section.is_some(), "hidden Section is ineligible");
    h.app.preferences.viewport_analysis_status = false;
    assert!(!crate::viewport_status::escape(&mut h.app, false));
    h.app.preferences.viewport_section_status = true;
    h.app.run("inspect.zebra", json!({})).unwrap();
    assert!(crate::viewport_status::escape(&mut h.app, false));
    assert!(h.app.session.analysis.is_some() && h.app.session.section.is_none(), "Section clears while analysis HUD is disabled");
    h.app.preferences.viewport_analysis_status = true;
    h.app.run("inspect.section", json!({"plane":"XY"})).unwrap();
    h.frames(3);
    let section = h.app.session.status_items().into_iter().find(|e| e.command == "inspect.section").unwrap();
    let item = crate::viewport_status::presentation(&h.app, &section).unwrap();
    h.step(&json!({"click":{"handle":format!("viewport_status:{}:close",item.id)}})).unwrap();
    assert!(h.app.session.analysis.is_some() && h.app.session.section.is_none());
}

#[test]
fn section_status_preserves_legacy_three_items_and_dialog_escape_ownership() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10})).unwrap();
    h.call("ui.prefs", json!({"single_surface_analysis":false}));
    h.app.run("inspect.zebra", json!({})).unwrap();
    h.app.run("inspect.continuity", json!({})).unwrap();
    h.app.run("inspect.section", json!({"plane":"XY","at":3})).unwrap();
    h.frames(3);
    let effects = h.app.session.status_items();
    assert_eq!(effects.len(), 3);
    for effect in &effects {
        let item = crate::viewport_status::presentation(&h.app, effect).unwrap();
        assert!(crate::scenario::rect_of(&format!("viewport_status:{}:item", item.id)).is_some());
    }
    crate::dialogs::edit_section(&mut h.app);
    h.frames(3);
    let cut = h.app.session.section;
    h.step(&json!({"key":"Escape"})).unwrap();
    assert!(h.app.dialog.is_none());
    assert_eq!(h.app.session.section, cut);
    h.step(&json!({"key":"Escape"})).unwrap();
    assert!(h.app.session.section.is_none());
    assert!(h.app.session.analysis.is_some() && !h.app.session.continuity.is_empty());
}

#[test]
fn section_close_ends_actual_cut_while_an_unrelated_tool_owns_escape() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10})).unwrap();
    h.app.start("inspect.continuity");
    h.app.run("inspect.section", json!({"plane":"XY","at":4})).unwrap();
    h.frames(3);
    assert!(h.app.tool.is_some());
    let effect = h.app.session.status_items().into_iter().find(|e| e.command == "inspect.section").unwrap();
    let item = crate::viewport_status::presentation(&h.app, &effect).unwrap();
    h.step(&json!({"click":{"handle":format!("viewport_status:{}:close",item.id)}})).unwrap();
    assert!(h.app.session.section.is_none());
    assert!(h.app.tool.is_some() && !h.app.session.continuity.is_empty());
}

#[test]
fn analysis_status_preferences_take_first_escape_and_release_the_next_frame() {
    let mut h = Harness::new();
    h.app.run("inspect.zebra", json!({})).unwrap();
    h.call("ui.prefs", json!({"open":"Interface"}));
    h.ctx.memory_mut(|memory| {
        if let Some(id) = memory.focused() {
            memory.surrender_focus(id);
        }
    });
    h.step(&json!({"key":"Escape"})).unwrap();
    assert!(!h.app.prefs_window.open);
    assert!(h.app.session.analysis.is_some(), "Preferences owns the first Escape even without text focus");
    h.step(&json!({"key":"Escape"})).unwrap();
    assert!(h.app.session.analysis.is_none(), "the next idle frame may end the effect");
}

#[test]
fn analysis_status_escape_owner_survives_egui_layout_repasses() {
    for owner in ["preferences", "command", "tool", "card", "search"] {
        let mut h = Harness::new();
        h.app.run("solid.box", json!({"length":10,"width":10,"height":10})).unwrap();
        h.app.run("inspect.draft", json!({})).unwrap();
        match owner {
            "preferences" => {
                h.call("ui.prefs", json!({"open":"Interface"}));
            }
            "command" => {
                h.app.start("solid.fillet");
                assert!(h.app.dialog.is_some());
            }
            "tool" => {
                h.app.start("inspect.continuity");
                assert!(h.app.tool.is_some());
            }
            "search" => {
                h.app.start("ui.palette");
            }
            _ => {
                h.frames(3);
                let effect = h.app.session.status_items().remove(0);
                let item = crate::viewport_status::presentation(&h.app, &effect).unwrap();
                h.step(&json!({"click":{"handle":format!("viewport_status:{}:key", item.id)}})).unwrap();
                assert!(h.app.viewport_status.open_key.is_some());
            }
        }
        h.frames(3);
        h.ctx.memory_mut(|memory| {
            if let Some(id) = memory.focused() {
                memory.surrender_focus(id);
            }
        });
        let before = h.app.session.status_items();
        let mut passes = 0;
        // One tap, like ui.key, rather than a held Escape that makes the next press a repeat.
        let mut escape = crate::test_support::key(egui::Key::Escape);
        escape.push(egui::Event::Key { key: egui::Key::Escape, physical_key: None, pressed: false, repeat: false, modifiers: Default::default() });
        let _output = h.ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(h.ctx.content_rect()),
                time: Some(h.ctx.input(|input| input.time) + 1.0 / 60.0),
                focused: true,
                events: escape,
                ..Default::default()
            },
            |ui| {
                passes += 1;
                h.app.logic(ui.ctx());
                h.app.ui(ui);
                if ui.ctx().current_pass_index() == 0 {
                    ui.ctx().request_discard("check Escape ownership over layout passes");
                }
            },
        );
        assert!(passes >= 2, "the frame must actually repeat its layout");
        assert_eq!(h.app.session.status_items(), before, "{owner} owns one Escape over every pass");
        assert!(!h.app.prefs_window.open && h.app.dialog.is_none() && h.app.tool.is_none() && !h.app.ui.palette_open);
        assert!(h.app.viewport_status.open_key.is_none());
        h.frame();
        h.step(&json!({"key":"Escape"})).unwrap();
        assert!(h.app.session.status_items().is_empty(), "{owner} releases the following idle Escape");
    }
}

#[test]
fn analysis_status_row_and_key_stay_below_preferences_and_cannot_steal_its_click() {
    for window_first in [false, true] {
        let mut h = Harness::new();
        if window_first {
            h.call("ui.prefs", json!({"open":"Interface"}));
        }
        h.app.run("inspect.draft", json!({})).unwrap();
        h.frames(3);
        let effect = h.app.session.status_items().remove(0);
        let item = crate::viewport_status::presentation(&h.app, &effect).unwrap();
        if !window_first {
            h.step(&json!({"click":{"handle":format!("viewport_status:{}:key", item.id)}})).unwrap();
            h.call("ui.prefs", json!({"open":"Interface"}));
        }
        h.frames(3);
        let window = crate::scenario::rect_of("prefs:window").unwrap();
        let mut targets = vec![format!("viewport_status:{}:close", item.id)];
        if !window_first {
            targets.push("viewport_status:card".into());
        }
        for target in targets {
            let at = crate::scenario::rect_of(&target).unwrap().center();
            assert!(window.contains(at), "the real Preferences window must overlap {target}");
            let layer = h.ctx.layer_id_at(at).unwrap();
            assert_eq!(layer.order, egui::Order::Middle, "Preferences stays above {target}");
            assert_ne!(layer.id, egui::Id::new("viewport_status_row"));
            assert_ne!(layer.id, egui::Id::new("viewport_status_key"));
        }
        let at = crate::scenario::handle_at(&format!("viewport_status:{}:close", item.id)).unwrap();
        h.call("ui.click", json!({"x":at.x,"y":at.y}));
        assert!(h.app.prefs_window.open);
        assert!(h.app.session.analysis.is_some(), "an occluded HUD close never receives the window's click");
    }
}

#[test]
fn analysis_status_focused_dialog_and_real_text_popup_preserve_owner_priority_in_both_policies() {
    for enabled in [false, true] {
        for popup in [false, true] {
            let mut h = Harness::new();
            h.app.preferences.viewport_section_status = false;
            h.call("ui.prefs", json!({"viewport_analysis_status":enabled}));
            h.app.run("solid.box", json!({"length":10,"width":10,"height":10})).unwrap();
            h.app.run("inspect.draft", json!({})).unwrap();
            h.app.start("solid.fillet");
            h.frames(3);
            let at = crate::scenario::handle_at("dialog:Radius").unwrap();
            h.call("ui.click", json!({"x":at.x,"y":at.y}));
            assert!(h.ctx.memory(|memory| memory.focused().is_some()), "the real radius editor has focus");
            if popup {
                h.call("ui.click", json!({"x":at.x,"y":at.y,"button":"right"}));
                assert!(egui::Popup::is_any_open(&h.ctx), "a real editor context menu is open");
                let mut reached_guard = false;
                let mut tap = crate::test_support::key(egui::Key::Escape);
                tap.push(egui::Event::Key {
                    key: egui::Key::Escape,
                    physical_key: None,
                    pressed: false,
                    repeat: false,
                    modifiers: Default::default(),
                });
                let _output = h.ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(h.ctx.content_rect()),
                        time: Some(h.ctx.input(|input| input.time) + 1.0 / 60.0),
                        focused: true,
                        events: tap,
                        ..Default::default()
                    },
                    |ui| {
                        let first = ui.ctx().current_pass_index() == 0;
                        if first {
                            assert!(egui::Popup::is_any_open(ui.ctx()), "the surviving popup predicate reaches the global guard");
                            reached_guard = true;
                        }
                        h.app.logic(ui.ctx());
                        if first {
                            assert!(h.app.dialog.is_some());
                            assert!(!h.app.esc_handled, "routing first refusal is not completed cancellation");
                            assert!(ui.ctx().input(|input| input.key_pressed(egui::Key::Escape)), "the real popup still receives its Escape");
                        }
                        h.app.ui(ui);
                    },
                );
                assert!(reached_guard);
                assert!(!egui::Popup::is_any_open(&h.ctx));
                assert!(h.app.dialog.is_some(), "the first Escape closes only the editor's menu");
                assert!(h.app.session.analysis.is_some());
            }
            // Plain TextEdit may blur at begin-pass: this asserts behavior, not guard capture.
            h.step(&json!({"key":"Escape"})).unwrap();
            assert!(h.app.dialog.is_none(), "the dialog's own Escape still cancels in policy {enabled}");
            assert!(h.app.session.analysis.is_some());
            h.step(&json!({"key":"Escape"})).unwrap();
            assert_eq!(h.app.session.analysis.is_none(), enabled, "the idle Escape follows the selected policy");
        }
    }
}

fn analysis_escape_relayout_frame(h: &mut Harness, mut first_pass: impl FnMut(&crate::SolveApp)) {
    let ctx = h.ctx.clone();
    let mut tap = crate::test_support::key(egui::Key::Escape);
    tap.push(egui::Event::Key { key: egui::Key::Escape, physical_key: None, pressed: false, repeat: false, modifiers: Default::default() });
    let mut passes = 0;
    let _output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(ctx.content_rect()),
            time: Some(ctx.input(|input| input.time) + 1.0 / 60.0),
            focused: true,
            events: tap,
            ..Default::default()
        },
        |ui| {
            passes += 1;
            h.app.logic(ui.ctx());
            h.app.ui(ui);
            if ui.ctx().current_pass_index() == 0 {
                first_pass(&h.app);
                ui.ctx().request_discard("check restored policy during the same Escape frame");
            }
        },
    );
    assert!(passes >= 2, "the policy transition must actually cross a layout re-pass");
}

#[test]
fn analysis_status_preferences_cancel_can_restore_either_policy_without_reusing_escape() {
    for saved in [false, true] {
        let mut h = Harness::new();
        h.app.preferences.viewport_section_status = false;
        h.call("ui.prefs", json!({"viewport_analysis_status":saved}));
        h.app.run("inspect.zebra", json!({})).unwrap();
        h.call("ui.prefs", json!({"open":"Interface"}));
        h.call("ui.prefs", json!({"viewport_analysis_status":!saved}));
        analysis_escape_relayout_frame(&mut h, |app| {
            assert!(!app.prefs_window.open);
            assert_eq!(app.preferences.viewport_analysis_status, saved, "Cancel restores policy in pass one");
            assert!(app.session.analysis.is_some());
        });
        assert_eq!(h.app.preferences.viewport_analysis_status, saved);
        assert!(h.app.session.analysis.is_some(), "Cancel owns this Escape through pass two while restoring policy {saved}");
        h.step(&json!({"key":"Escape"})).unwrap();
        assert_eq!(h.app.session.analysis.is_none(), saved);
    }
}

#[test]
fn analysis_status_preferences_restore_false_keeps_underlying_dialog_through_the_owned_frame() {
    let mut h = Harness::new();
    h.app.preferences.viewport_section_status = false;
    h.call("ui.prefs", json!({"viewport_analysis_status":false}));
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10})).unwrap();
    h.app.run("inspect.draft", json!({})).unwrap();
    h.app.start("solid.fillet");
    assert!(h.app.dialog.is_some());
    h.call("ui.prefs", json!({"open":"Interface"}));
    h.call("ui.prefs", json!({"viewport_analysis_status":true}));
    analysis_escape_relayout_frame(&mut h, |app| {
        assert!(!app.prefs_window.open);
        assert!(!app.preferences.viewport_analysis_status, "Cancel restores false before pass two");
        assert!(app.dialog.is_some(), "Preferences shields the existing command dialog in pass one");
    });
    assert!(h.app.dialog.is_some(), "the same Escape must not cancel the underlying dialog in pass two");
    assert!(h.app.session.analysis.is_some());
    h.step(&json!({"key":"Escape"})).unwrap();
    assert!(h.app.dialog.is_none(), "the next physical Escape belongs to the command dialog");
    assert!(h.app.session.analysis.is_some());
    h.step(&json!({"key":"Escape"})).unwrap();
    assert!(h.app.session.analysis.is_some(), "the restored legacy policy keeps shading on idle Escape");
}

/// The countdown ring around a toast's × (#7971) is the share of its time left: whole when it
/// starts, half at half time, gone when it is over, and whole for times no clock gives.
#[test]
fn the_countdown_ring_follows_the_time_left() {
    assert_eq!(countdown_left(10.0, 10.0, 6.0), 1.0);
    assert!((countdown_left(10.0, 13.0, 6.0) - 0.5).abs() < 1e-6);
    assert!((countdown_left(0.0, 1.5, 6.0) - 0.75).abs() < 1e-6);
    assert_eq!(countdown_left(10.0, 16.0, 6.0), 0.0);
    assert_eq!(countdown_left(10.0, 99.0, 6.0), 0.0);
    assert_eq!(countdown_left(10.0, 5.0, 6.0), 1.0, "a start in the future");
    for (since, now, lifetime) in [(f64::NAN, 1.0, 6.0), (0.0, f64::INFINITY, f64::INFINITY), (0.0, 1.0, 0.0), (0.0, 0.0, 0.0)] {
        let left = countdown_left(since, now, lifetime);
        assert!((0.0..=1.0).contains(&left), "{since} {now} {lifetime}: {left}");
    }
    // The arc: from twelve o'clock, clockwise (y down), over that share of a turn, on the ring.
    let centre = pos2(100.0, 50.0);
    for left in [1.0_f32, 0.75, 0.5, 0.25, 0.01] {
        let arc = countdown_arc(centre, 10.0, left);
        assert!(arc.len() >= 2, "{left}");
        assert!(arc.iter().all(|p| ((*p - centre).length() - 10.0).abs() < 1e-3), "{left}: on the ring");
        assert!((arc[0] - pos2(100.0, 40.0)).length() < 1e-3, "starts at twelve o'clock");
        assert!((swept(&arc, centre) - left).abs() < 1e-3, "{left}: sweeps {}", swept(&arc, centre));
    }
    assert!((countdown_arc(centre, 10.0, 0.25).last().copied().unwrap() - pos2(110.0, 50.0)).length() < 1e-3, "a quarter ends at three o'clock");
    for bad in [0.0_f32, -1.0, f32::NAN] {
        assert!(countdown_arc(centre, 10.0, bad).is_empty(), "{bad}");
    }
    assert!(countdown_arc(centre, f32::NAN, 0.5).is_empty() && countdown_arc(centre, 0.0, 0.5).is_empty());
}

/// The share of a turn an arc of `points` sweeps clockwise (y down) round `centre`.
fn swept(points: &[Pos2], centre: Pos2) -> f32 {
    let turn = std::f32::consts::TAU;
    points.windows(2).map(|w| ((w[1] - centre).angle() - (w[0] - centre).angle()).rem_euclid(turn)).sum::<f32>() / turn
}

/// The status toast has a close (×) at its top right and no Close button (#7971): the × closes it
/// without opening the run's messages (a click elsewhere on it still does), its ring shows the
/// share of its six seconds left, and behind an issue report the × does nothing.
#[test]
fn the_status_toast_closes_with_its_x_and_shows_its_time_left() {
    let t = Tokens::get();
    let mut h = Harness::new();
    h.app.home.open = false;
    // Half its time gone.
    h.app.status = Some(("Opened".into(), crate::now_ms() - STATUS_TOAST_MS / 2.0, false));
    h.frames(2);
    let toast = crate::scenario::rect_of("toast").expect("the status toast");
    let close = crate::scenario::handle_at("toast:status:close").expect("its ×");
    assert!(toast.contains(close) && close.x > toast.center().x && close.y <= toast.center().y + 1.0, "top right: {close:?} in {toast:?}");
    fn ring(shape: &Shape, ink: Color32) -> Option<Vec<Pos2>> {
        match shape {
            Shape::Path(p) if !p.closed && p.stroke.color == egui::epaint::ColorMode::Solid(ink) && p.points.len() >= 2 => Some(p.points.clone()),
            Shape::Vec(shapes) => shapes.iter().find_map(|s| ring(s, ink)),
            _ => None,
        }
    }
    let arc = h.shapes.iter().find_map(|s| ring(&s.shape, t.text_dim)).expect("the ring is drawn");
    assert!((swept(&arc, close) - 0.5).abs() < 0.05, "about half a turn left: {}", swept(&arc, close));
    assert!(h.widget("Close").is_some(), "the × has an accessible name");
    // Behind an issue report it does nothing.
    crate::issue_report::set_cad_input_blocked_for_test(true);
    h.step(&json!({"click": {"handle": "toast:status:close"}})).unwrap();
    h.frames(2);
    crate::issue_report::set_cad_input_blocked_for_test(false);
    assert!(h.app.status.is_some(), "not closed behind a report");
    h.step(&json!({"click": {"handle": "toast:status:close"}})).unwrap();
    h.frames(2);
    assert!(h.app.status.is_none(), "closed");
    assert!(!h.app.messages.open, "the × doesn't open the messages");
    assert!(crate::scenario::rect_of("toast").is_none());
    // A click elsewhere on it still opens them.
    h.app.status = Some(("Opened".into(), crate::now_ms(), false));
    h.frames(2);
    h.step(&json!({"click": {"handle": "status:chip"}})).unwrap();
    h.frames(2);
    assert!(h.app.messages.open, "the card opens the messages");
}

#[test]
fn section_only_status_modal_owners_take_first_escape_across_real_layout_repasses() {
    for owner in ["preferences", "command", "tool", "search"] {
        let mut h = Harness::new();
        h.call("ui.prefs", json!({"viewport_analysis_status":false,"viewport_section_status":true}));
        h.app.run("solid.box", json!({"length":10,"width":10,"height":10})).unwrap();
        h.app.run("inspect.section", json!({"plane":"XY","at":4})).unwrap();
        match owner {
            "preferences" => {
                h.call("ui.prefs", json!({"open":"Interface"}));
            }
            "command" => h.app.start("solid.fillet"),
            "tool" => h.app.start("inspect.continuity"),
            _ => h.app.start("ui.palette"),
        }
        h.frames(3);
        h.ctx.memory_mut(|memory| {
            if let Some(id) = memory.focused() {
                memory.surrender_focus(id);
            }
        });
        let cut = h.app.session.section;
        analysis_escape_relayout_frame(&mut h, |app| {
            assert_eq!(app.session.section, cut, "{owner} owns the tap in pass one");
            assert!(!app.prefs_window.open && app.dialog.is_none() && app.tool.is_none() && !app.ui.palette_open);
        });
        assert_eq!(h.app.session.section, cut, "{owner} keeps the cut through the discarded frame");
        h.step(&json!({"key":"Escape"})).unwrap();
        assert!(h.app.session.section.is_none(), "a distinct idle tap clears Section after {owner}");
    }
}

#[test]
fn section_only_status_real_editor_popup_then_dialog_own_escape_before_the_cut() {
    let mut h = Harness::new();
    h.call("ui.prefs", json!({"viewport_analysis_status":false,"viewport_section_status":true}));
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10})).unwrap();
    h.app.run("inspect.section", json!({"plane":"XY","at":4})).unwrap();
    h.app.start("solid.fillet");
    h.frames(3);
    let at = crate::scenario::handle_at("dialog:Radius").unwrap();
    h.call("ui.click", json!({"x":at.x,"y":at.y}));
    assert!(h.ctx.memory(|memory| memory.focused().is_some()));
    h.call("ui.click", json!({"x":at.x,"y":at.y,"button":"right"}));
    assert!(egui::Popup::is_any_open(&h.ctx));
    let cut = h.app.session.section;
    analysis_escape_relayout_frame(&mut h, |app| {
        assert!(app.dialog.is_some(), "the real popup has first refusal before Dialog cancellation");
        assert_eq!(app.session.section, cut);
    });
    assert!(!egui::Popup::is_any_open(&h.ctx));
    assert!(h.app.dialog.is_some());
    h.step(&json!({"key":"Escape"})).unwrap();
    assert!(h.app.dialog.is_none());
    assert_eq!(h.app.session.section, cut, "the distinct second tap belongs to the command dialog");
    h.step(&json!({"key":"Escape"})).unwrap();
    assert!(h.app.session.section.is_none());
}

#[test]
fn section_and_analysis_key_release_distinct_idle_escape_to_the_newest_cut() {
    let mut h = Harness::new();
    h.app.run("inspect.draft", json!({})).unwrap();
    h.app.run("inspect.section", json!({"plane":"XY","at":4})).unwrap();
    h.frames(3);
    let effect = h.app.session.status_items().into_iter().find(|effect| effect.has_key).unwrap();
    let item = crate::viewport_status::presentation(&h.app, &effect).unwrap();
    h.step(&json!({"click":{"handle":format!("viewport_status:{}:key",item.id)}})).unwrap();
    assert!(h.app.viewport_status.open_key.is_some());
    let cut = h.app.session.section;
    analysis_escape_relayout_frame(&mut h, |app| {
        assert!(app.viewport_status.open_key.is_none());
        assert_eq!(app.session.section, cut, "a coexisting analysis key owns the first tap");
    });
    assert!(h.app.session.analysis.is_some());
    h.step(&json!({"key":"Escape"})).unwrap();
    assert!(h.app.session.section.is_none() && h.app.session.analysis.is_some());
}

#[test]
fn section_only_preferences_cancel_restores_either_policy_and_shields_existing_dialog() {
    for saved in [false, true] {
        let mut h = Harness::new();
        h.call("ui.prefs", json!({"viewport_analysis_status":false,"viewport_section_status":saved}));
        h.app.run("solid.box", json!({"length":10,"width":10,"height":10})).unwrap();
        h.app.run("inspect.section", json!({"plane":"XY","at":4})).unwrap();
        if !saved {
            h.app.start("solid.fillet");
        }
        h.call("ui.prefs", json!({"open":"Interface"}));
        h.call("ui.prefs", json!({"viewport_section_status":!saved}));
        let cut = h.app.session.section;
        analysis_escape_relayout_frame(&mut h, |app| {
            assert!(!app.prefs_window.open);
            assert_eq!(app.preferences.viewport_section_status, saved);
            assert!(!app.preferences.viewport_analysis_status);
            assert_eq!(app.dialog.is_some(), !saved, "enabled routing protects the existing form in pass one");
            assert_eq!(app.session.section, cut);
        });
        assert_eq!(h.app.dialog.is_some(), !saved, "no tap is reinserted during the actual second pass");
        if !saved {
            h.step(&json!({"key":"Escape"})).unwrap();
            assert!(h.app.dialog.is_none());
            assert_eq!(h.app.session.section, cut);
        }
        h.step(&json!({"key":"Escape"})).unwrap();
        assert_eq!(h.app.session.section.is_none(), saved, "both-false restores the actual legacy idle policy");
    }
}

#[test]
fn section_only_status_stays_below_preferences_for_paint_and_hit_testing() {
    let mut h = Harness::new();
    h.call("ui.prefs", json!({"viewport_analysis_status":false,"viewport_section_status":true}));
    h.app.run("inspect.section", json!({"plane":"XY","at":4})).unwrap();
    h.frames(3);
    h.call("ui.prefs", json!({"open":"Interface"}));
    h.frames(3);
    let effect = h.app.session.status_items().into_iter().find(|e| e.command == "inspect.section").unwrap();
    let item = crate::viewport_status::presentation(&h.app, &effect).unwrap();
    let at = crate::scenario::handle_at(&format!("viewport_status:{}:close", item.id)).unwrap();
    assert!(crate::scenario::rect_of("prefs:window").unwrap().contains(at));
    assert_eq!(h.ctx.layer_id_at(at).unwrap().order, egui::Order::Middle);
    h.call("ui.click", json!({"x":at.x,"y":at.y}));
    assert!(h.app.prefs_window.open && h.app.session.section.is_some());
}
