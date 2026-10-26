use super::*;
use crate::{scenario::Harness, shell::LayoutMode};

// These tests inspect shapes without submitting their texture uploads to a renderer.
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
                    let free = crate::viewport_layout::free_rect(content, &obstacles).shrink(Tokens::get().space_8);
                    if let Some(toast) = crate::scenario::rect_of("toast") {
                        assert!(toast.is_finite() && toast.is_positive());
                        assert!(content.contains_rect(toast), "{mode:?}, {scale}, {width}x{height}: {toast:?}");
                        for obstacle in obstacles {
                            assert!(!toast.intersect(obstacle).is_positive(), "{toast:?} intersects {obstacle:?}");
                        }
                        assert!((toast.center().x - free.center().x).abs() < 0.01);
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

/// The status and computing toasts keep the size and place they always had: the text's size plus
/// 40 by 16, centred at the bottom of the free rect.
#[test]
fn the_shared_toast_draws_old_callers_exactly_as_before() {
    let ctx = crate::fonts::test_context();
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
    let labels = [
        ("Calibrate…", ToastKind::Primary),
        ("Swap to Hidraw (Direct Driver)", ToastKind::Secondary),
        ("Close", ToastKind::Dismiss),
        ("Never show again", ToastKind::Dismiss),
    ];
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
            let (card, _) = run(vec![]).unwrap_or_else(|| panic!("width {width}: no card"));
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
                    if let Some((_, Some(index))) = clicked {
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
                assert_eq!(hits, [0, 1, 2, 3], "width {width}: every button can be clicked inside the card");
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
    h.app.home.open = false;
    h.app.ui.layout = LayoutMode::Floating;
    h.app.preferences.timeline_left = true;
    h.app.preferences.show_frame_rate = true;
    h.app.ui.show_timeline = true;
    h.app.run("inspect.draft", json!({})).unwrap();
    h.frame();
    let viewport = h.app.viewport.layout.as_ref().unwrap().viewport;
    for name in ["analysis:legend", "viewport:frame_rate"] {
        let readout = crate::scenario::rect_of(name).unwrap();
        assert!((readout.bottom() - (viewport.bottom() - 12.0)).abs() < 0.01, "{name}: {readout:?} in {viewport:?}");
    }
}

/// The edges of `body` whose ends are `a` and `b`, either way round.
fn edge_between(h: &Harness, a: Vec3, b: Vec3) -> Sel {
    let st = h.app.session.world_state();
    let body = st.bodies.first().unwrap();
    let m = body.mesh();
    let index = m
        .edges
        .iter()
        .position(|e| {
            let (p, q) = (*e.first().unwrap(), *e.last().unwrap());
            (p.dist(a) < 1e-6 && q.dist(b) < 1e-6) || (p.dist(b) < 1e-6 && q.dist(a) < 1e-6)
        })
        .unwrap();
    Sel::Edge { body: body.name.clone(), index, point: (a + b) * 0.5 }
}

#[test]
fn selected_edges_show_their_length_at_the_bottom_left() {
    // #6560: an edge's length without opening Measure.
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.run("solid.box", json!({"length":40,"width":30,"height":20})).unwrap();
    let x = edge_between(&h, Vec3::ZERO, Vec3::new(40.0, 0.0, 0.0));
    let y = edge_between(&h, Vec3::ZERO, Vec3::new(0.0, 30.0, 0.0));
    h.app.session.selection = vec![x.clone()];
    let one = crate::i18n::format("selection.length", &[("length", &crate::prefs::show_mm(&h.app, 40.0, 1))]);
    assert_eq!(crate::selection_length::text(&h.app), Some(one));
    h.frame();
    let viewport = h.app.viewport.layout.as_ref().unwrap().viewport;
    let readout = crate::scenario::rect_of("viewport:selection_length").unwrap();
    assert!(viewport.contains_rect(readout), "{readout:?} in {viewport:?}");

    h.app.session.selection = vec![x.clone(), y];
    let total = crate::i18n::format("selection.length_total", &[("length", &crate::prefs::show_mm(&h.app, 70.0, 1)), ("count", "2")]);
    assert_eq!(crate::selection_length::text(&h.app), Some(total));

    // Anything that isn't an edge or a curve has no length to show.
    let body = h.app.session.world_state().bodies.first().unwrap().name.clone();
    let face = Sel::Face { body, index: 0, point: Vec3::new(20.0, 15.0, 0.0) };
    h.app.session.selection = vec![x.clone(), face];
    assert_eq!(crate::selection_length::text(&h.app), None);

    // With an analysis legend up, the readout sits above it.
    h.app.run("inspect.draft", json!({})).unwrap();
    h.app.session.selection = vec![x];
    h.frame();
    let legend = crate::scenario::rect_of("analysis:legend").unwrap();
    let readout = crate::scenario::rect_of("viewport:selection_length").unwrap();
    assert!(readout.bottom() <= legend.top(), "{readout:?} overlaps {legend:?}");
}

#[test]
fn a_circle_picked_as_both_rim_halves_is_one_edge() {
    let mut h = Harness::new();
    h.app.run("solid.cylinder", json!({"radius": 10, "height": 20})).unwrap();
    let st = h.app.session.world_state();
    let body = st.bodies.first().unwrap();
    let m = body.mesh();
    let rim: Vec<Sel> = m
        .edges
        .iter()
        .enumerate()
        .filter(|(_, p)| p.len() > 3 && p.iter().all(|q| (q.z - 20.0).abs() < 1e-6))
        .map(|(index, p)| Sel::Edge { body: body.name.clone(), index, point: polyline_mid(p) })
        .collect();
    assert!(!rim.is_empty());
    let (length, count) = crate::selection_length::measure(&h.app, &st, &rim).unwrap();
    assert_eq!(count, 1);
    assert!((length - 20.0 * std::f64::consts::PI).abs() < 0.2, "{length}");
}

#[test]
fn sketch_curves_show_their_length_and_dialogs_hide_it() {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
    h.app.run("sketch.line", json!({"points": [[0, 0], [30, 40]]})).unwrap();
    h.app.session.selection = vec![Sel::SketchCurve { id: "l1".into(), sketch: None }];
    let expected = crate::i18n::format("selection.length", &[("length", &crate::prefs::show_mm(&h.app, 50.0, 1))]);
    assert_eq!(crate::selection_length::text(&h.app), Some(expected));
    h.frame();
    assert!(crate::scenario::rect_of("viewport:selection_length").is_some());
    h.app.start("inspect.measure");
    h.frame();
    assert!(crate::scenario::rect_of("viewport:selection_length").is_none(), "Measure shows its own values");
}
