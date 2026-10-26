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

fn grid_texts(h: &Harness) -> Vec<(String, Pos2)> {
    let mut out = Vec::new();
    fn walk(shape: &Shape, out: &mut Vec<(String, Pos2)>) {
        match shape {
            Shape::Text(text) => out.push((text.galley.text().to_string(), text.pos + text.galley.size() / 2.0)),
            Shape::Vec(shapes) => shapes.iter().for_each(|shape| walk(shape, out)),
            _ => {}
        }
    }
    for clipped in &h.shapes {
        walk(&clipped.shape, &mut out);
    }
    out
}

fn look_straight_down(h: &mut Harness, distance: f64) {
    h.app.home.open = false;
    h.app.cancel_view_animation();
    h.app.cam.set_view(solidcraft_engine::render::StandardView::Top);
    h.app.cam.target = Vec3::ZERO;
    h.app.cam.distance = distance;
    h.app.cam.fov = 0.0;
    h.frame();
}

fn mark_near(h: &Harness, label: &str, world: Vec3) -> bool {
    let Some(rect) = h.app.viewport.rect else { return false };
    let Some(at) = projection(&h.app, rect).to_screen(world) else { return false };
    grid_texts(h).iter().any(|(text, pos)| text == label && pos.distance(at) < 20.0)
}

/// The half-transparent plate drawn on the sketch plane nearest `world`.
fn grid_mark_box(h: &Harness, world: Vec3) -> Option<std::sync::Arc<egui::Mesh>> {
    let rect = h.app.viewport.rect?;
    let at = projection(&h.app, rect).to_screen(world)?;
    let mut best: Option<(f32, std::sync::Arc<egui::Mesh>)> = None;
    fn walk(shape: &Shape, at: Pos2, best: &mut Option<(f32, std::sync::Arc<egui::Mesh>)>) {
        match shape {
            // The fill is half transparent. Feathered edges are fully clear, so the plate is
            // the mesh that reaches alpha 128 and never goes above it.
            Shape::Mesh(mesh)
                if mesh.vertices.len() >= 4
                    && mesh.vertices.iter().any(|v| v.color.a() == 128)
                    && mesh.vertices.iter().all(|v| v.color.a() <= 128) =>
            {
                let n = mesh.vertices.len() as f32;
                let centre = mesh.vertices.iter().fold(egui::Vec2::ZERO, |sum, v| sum + v.pos.to_vec2()) / n;
                let distance = centre.to_pos2().distance(at);
                if distance < 40.0 && best.as_ref().is_none_or(|(so_far, _)| distance < *so_far) {
                    *best = Some((distance, std::sync::Arc::clone(mesh)));
                }
            }
            Shape::Vec(shapes) => shapes.iter().for_each(|shape| walk(shape, at, best)),
            _ => {}
        }
    }
    for clipped in &h.shapes {
        walk(&clipped.shape, at, &mut best);
    }
    best.map(|(_, mesh)| mesh)
}

/// How closely the plate's long direction matches `axis` on screen. A screen-aligned plate
/// stays horizontal; one locked to the sketch plane follows the axis as the view tilts.
fn plane_alignment(h: &Harness, world: Vec3, axis: Vec3) -> Option<f32> {
    let mesh = grid_mark_box(h, world)?;
    let n = mesh.vertices.len() as f32;
    let centre = mesh.vertices.iter().fold(egui::Vec2::ZERO, |sum, v| sum + v.pos.to_vec2()) / n;
    let (mut xx, mut xy, mut yy) = (0.0_f32, 0.0, 0.0);
    for v in &mesh.vertices {
        let d = v.pos.to_vec2() - centre;
        xx += d.x * d.x;
        xy += d.x * d.y;
        yy += d.y * d.y;
    }
    let trace = xx + yy;
    let det = xx * yy - xy * xy;
    let lambda = 0.5 * (trace + (trace * trace - 4.0 * det).max(0.0).sqrt());
    let long = if xy.abs() > 1e-3 {
        vec2(xy, lambda - xx)
    } else if xx >= yy {
        vec2(1.0, 0.0)
    } else {
        vec2(0.0, 1.0)
    };
    let long = long / long.length().max(1e-6);
    let rect = h.app.viewport.rect?;
    let proj = projection(&h.app, rect);
    let screen = proj.to_screen(world + axis)? - proj.to_screen(world)?;
    let screen = screen / screen.length().max(1e-6);
    Some((long.x * screen.x + long.y * screen.y).abs())
}

/// The axes show the major step in the design's units. Zooming out coarsens that step.
#[test]
fn grid_numbers_follow_zoom_and_the_design_units() {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
    look_straight_down(&mut h, 200.0);
    assert!(mark_near(&h, "25", Vec3::new(25.0, 0.0, 0.0)), "x: {:?}", grid_texts(&h));
    assert!(mark_near(&h, "-50", Vec3::new(-50.0, 0.0, 0.0)), "x negative: {:?}", grid_texts(&h));
    assert!(mark_near(&h, "25", Vec3::new(0.0, 25.0, 0.0)), "y: {:?}", grid_texts(&h));
    assert!(!mark_near(&h, "0", Vec3::ZERO), "the origin is not numbered");
    let plate = grid_mark_box(&h, Vec3::new(25.0, 0.0, 0.0)).expect("the number sits on a plate");
    assert!(plate.vertices.iter().any(|v| v.color.a() == 128) && plate.vertices.iter().all(|v| v.color.a() <= 128), "the plate is half transparent");

    h.app.cancel_view_animation();
    h.app.cam.set_view(solidcraft_engine::render::StandardView::Iso);
    h.app.cam.target = Vec3::ZERO;
    h.app.cam.distance = 200.0;
    h.app.cam.fov = 0.0;
    h.frame();
    assert!(mark_near(&h, "25", Vec3::new(25.0, 0.0, 0.0)), "angled view stays on the sketch plane: {:?}", grid_texts(&h));
    assert!(mark_near(&h, "25", Vec3::new(0.0, 25.0, 0.0)), "angled y: {:?}", grid_texts(&h));
    let along_x = plane_alignment(&h, Vec3::new(-50.0, 0.0, 0.0), Vec3::X);
    let along_y = plane_alignment(&h, Vec3::new(0.0, 100.0, 0.0), Vec3::Y);
    assert!(along_x.is_some_and(|align| align > 0.95), "the x plate follows the axis, not the screen: {along_x:?}");
    assert!(along_y.is_some_and(|align| align > 0.95), "the y plate follows the axis, not the screen: {along_y:?}");

    look_straight_down(&mut h, 800.0);
    assert!(mark_near(&h, "100", Vec3::new(100.0, 0.0, 0.0)), "zoomed out: {:?}", grid_texts(&h));
    assert!(!mark_near(&h, "25", Vec3::new(25.0, 0.0, 0.0)), "the fine step drops out");

    h.app.run("document.units", json!({"units": "in"})).unwrap();
    look_straight_down(&mut h, 200.0);
    assert!(mark_near(&h, "1", Vec3::new(25.4, 0.0, 0.0)), "inches: {:?}", grid_texts(&h));
    assert!(mark_near(&h, "-2", Vec3::new(-50.8, 0.0, 0.0)), "inches: {:?}", grid_texts(&h));
    assert!(!mark_near(&h, "25", Vec3::new(25.0, 0.0, 0.0)), "millimetre numbers leave with the units");
}

/// Turning the view never mirrors a number or turns it upside down. The plate still follows
/// its axis, including when the sketch plane is seen from the back.
#[test]
fn grid_numbers_stay_upright_as_the_view_turns() {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.cam.fov = 0.0;
    h.app.cam.distance = 200.0;
    h.app.cam.target = Vec3::ZERO;
    h.app.cam.roll = 0.0;
    look_straight_down(&mut h, 200.0);
    let rect = h.app.viewport.rect.expect("viewport");
    let mm = 2.0 * h.app.cam.half_height() / f64::from(rect.height());
    for plane in [solidcraft_engine::geom::Plane::XY, solidcraft_engine::geom::Plane::XZ, solidcraft_engine::geom::Plane::YZ] {
        for step in 0..12 {
            let yaw = f64::from(step) * std::f64::consts::TAU / 12.0;
            for pitch in [-1.2, -0.45, 0.2, 0.7, 1.2] {
                h.app.cam.yaw = yaw;
                h.app.cam.pitch = pitch;
                let proj = projection(&h.app, rect);
                for axis in [plane.x, plane.y] {
                    let Some(side) = plane.normal().cross(axis).normalized() else { continue };
                    let Some((_, right, down)) = plane_label_frame(&proj, plane.origin + axis * 50.0, axis, side, mm, 12.0) else {
                        continue;
                    };
                    let det = right.x * down.y - right.y * down.x;
                    assert!(det > 0.0, "{plane:?} yaw {yaw} pitch {pitch} mirrors the digits: right {right:?} down {down:?}");
                    assert!(down.y >= -1e-3, "{plane:?} yaw {yaw} pitch {pitch} turns the digits over: right {right:?} down {down:?}");
                }
            }
        }
    }
}

/// The model grid stays unlabeled. Numbers belong to the open sketch.
#[test]
fn model_view_hides_grid_numbers() {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.ui.show_grid = true;
    look_straight_down(&mut h, 200.0);
    assert!(h.app.session.active_sketch.is_none());
    assert!(!mark_near(&h, "-50", Vec3::new(-50.0, 0.0, 0.0)), "{:?}", grid_texts(&h));
    assert!(!mark_near(&h, "25", Vec3::new(0.0, 25.0, 0.0)), "{:?}", grid_texts(&h));
}
