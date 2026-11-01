use super::*;
use crate::ui_kit::test_support::pointer;

fn frame<R>(ctx: &egui::Context, events: Vec<egui::Event>, mut add: impl FnMut(&mut Ui) -> R) {
    crate::ui_kit::test_support::frame(ctx, events, |ui| {
        add(ui);
    });
}

fn click(ctx: &egui::Context, key: &str, rgb: &mut [u8; 3]) {
    let at = scenario::handle_at(key).unwrap_or_else(|| panic!("missing {key}"));
    frame(ctx, pointer(at, true), |ui| color_popover(ui, rgb));
    frame(ctx, pointer(at, false), |ui| color_popover(ui, rgb));
}

fn paste_hex(ctx: &egui::Context, rgb: &mut [u8; 3], text: &str) -> bool {
    click(ctx, "color:hex", rgb);
    let modifiers = egui::Modifiers { command: true, ctrl: true, ..Default::default() };
    let mut changed = false;
    frame(
        ctx,
        vec![
            egui::Event::Key { key: egui::Key::A, physical_key: None, pressed: true, repeat: false, modifiers },
            egui::Event::Key { key: egui::Key::A, physical_key: None, pressed: false, repeat: false, modifiers },
            egui::Event::Paste(text.into()),
        ],
        |ui| changed |= color_popover(ui, rgb),
    );
    changed
}

fn copy(ctx: &egui::Context, rgb: &mut [u8; 3]) -> Option<String> {
    let at = scenario::handle_at("color:copy").unwrap();
    frame(ctx, pointer(at, true), |ui| color_popover(ui, rgb));
    let output = crate::test_support::frame(
        ctx,
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(320.0, 400.0))),
            events: pointer(at, false),
            focused: true,
            ..Default::default()
        },
        |ui| {
            ui.set_width(280.0);
            crate::ui_kit::scope(ui, |ui| color_popover(ui, rgb));
        },
    );
    output.platform_output.commands.into_iter().find_map(|command| match command {
        egui::OutputCommand::CopyText(text) => Some(text),
        _ => None,
    })
}

#[test]
fn linear_field_preserves_hdr_precision_without_edits_and_bounds_hostile_limits() {
    let ctx = crate::fonts::test_context();
    let original = [0.123456789, 18.0, 0.9];
    let mut rgb = original;
    for maximum in [1e6, f64::NAN, f64::INFINITY] {
        frame(&ctx, vec![], |ui| linear_color_field(ui, &mut rgb, maximum));
        assert_eq!(rgb, original);
    }
}

#[test]
fn format_toggle_is_global_and_does_not_change_colour() {
    let ctx = crate::fonts::test_context();
    let mut rgb = [204, 208, 214];
    frame(&ctx, vec![], |ui| color_popover(ui, &mut rgb));
    assert_eq!(ctx.style_of(egui::Theme::Light).visuals.numeric_color_space, egui::style::NumericColorSpace::GammaByte);
    assert!(scenario::handle_at("color:channel:R").is_some());
    assert!(scenario::handle_at("color:hex").is_none());
    click(&ctx, "color:format:F", &mut rgb);
    assert_eq!(ctx.style_of(egui::Theme::Light).visuals.numeric_color_space, egui::style::NumericColorSpace::Linear);
    assert_eq!(rgb, [204, 208, 214]);
    click(&ctx, "color:format:Hex", &mut rgb);
    let mut other = [1, 2, 3];
    frame(&ctx, vec![], |ui| ui.push_id("another_picker", |ui| color_popover(ui, &mut other)));
    assert!(scenario::handle_at("color:hex").is_some(), "Hex selection is shared with other pickers");
    for name in ["R", "G", "B"] {
        assert!(scenario::handle_at(&format!("color:channel:{name}")).is_none());
    }
    for key in ["color:sv", "color:hue", "color:current"] {
        assert!(scenario::rect_of(key).is_some(), "Hex retains {key}");
    }
    assert_eq!(other, [1, 2, 3]);
    frame(&ctx, vec![], |ui| color_popover(ui, &mut rgb));
    click(&ctx, "color:format:U8", &mut rgb);
    assert_eq!(ctx.style_of(egui::Theme::Light).visuals.numeric_color_space, egui::style::NumericColorSpace::GammaByte);
    assert_eq!(rgb, [204, 208, 214]);
    assert!(scenario::handle_at("color:channel:R").is_some());
    assert!(scenario::handle_at("color:hex").is_none());
}

#[test]
fn hex_edits_apply_complete_byte_values_and_reject_invalid_or_nonopaque_pastes() {
    let ctx = crate::fonts::test_context();
    let mut rgb = [100, 120, 140];
    frame(&ctx, vec![], |ui| color_popover(ui, &mut rgb));
    click(&ctx, "color:format:Hex", &mut rgb);
    for (text, expected) in [
        ("#0102fe", [1, 2, 254]),
        ("#fE8001", [254, 128, 1]),
        ("#00ff01", [0, 255, 1]),
        ("#000000", [0, 0, 0]),
        ("#ffffff", [255, 255, 255]),
        ("#1b2C3DfF", [27, 44, 61]),
    ] {
        assert!(paste_hex(&ctx, &mut rgb, text), "{text} applies without Enter");
        assert_eq!(rgb, expected, "{text} retains exact bytes without a colour-space round trip");
    }
    for text in [
        "",
        "#",
        "#12345",
        "#1234567",
        "123456",
        "##123456",
        "#12345g",
        "#+102fe",
        "#0102+e",
        "#é1234",
        "#123456fff",
        "#123456 ",
        "#112233ffignored",
        "#fedcba80",
        "#fedcba00",
    ] {
        assert!(!paste_hex(&ctx, &mut rgb, text), "{text:?} is not an accepted colour edit");
        frame(&ctx, vec![], |ui| color_popover(ui, &mut rgb));
        assert_eq!(rgb, [27, 44, 61], "{text:?} must not apply even its RGB part");
    }
    assert_eq!(copy(&ctx, &mut rgb).map(|text| text.to_ascii_lowercase()), Some("#1b2c3dff".into()));

    assert!(!paste_hex(&ctx, &mut rgb, "#12"));
    frame(&ctx, vec![], |ui| color_popover(ui, &mut rgb));
    frame(&ctx, vec![egui::Event::Text("3456".into())], |ui| color_popover(ui, &mut rgb));
    assert_eq!(rgb, [18, 52, 86], "an incomplete draft survives idle frames and remains editable");
}

#[test]
fn hex_text_and_copy_follow_picker_and_external_colour_edits() {
    let ctx = crate::fonts::test_context();
    let mut rgb = [255, 0, 0];
    frame(&ctx, vec![], |ui| color_popover(ui, &mut rgb));
    click(&ctx, "color:format:Hex", &mut rgb);
    assert!(!paste_hex(&ctx, &mut rgb, "#12"));
    click(&ctx, "color:hue", &mut rgb);
    assert_ne!(rgb, [255, 0, 0]);
    let [r, g, b] = rgb;
    assert_eq!(copy(&ctx, &mut rgb).map(|text| text.to_ascii_lowercase()), Some(format!("#{r:02x}{g:02x}{b:02x}")));
    rgb = [240, 128, 17];
    frame(&ctx, vec![], |ui| color_popover(ui, &mut rgb));
    assert_eq!(copy(&ctx, &mut rgb).map(|text| text.to_ascii_lowercase()), Some("#f08011".into()));
    assert!(!paste_hex(&ctx, &mut rgb, "#f08011ff"), "changing representation alone is not a colour edit");
    click(&ctx, "color:format:F", &mut rgb);
    click(&ctx, "color:format:U8", &mut rgb);
    click(&ctx, "color:format:Hex", &mut rgb);
    assert_eq!(copy(&ctx, &mut rgb).map(|text| text.to_ascii_lowercase()), Some("#f08011ff".into()));
    assert_eq!(rgb, [240, 128, 17], "mode switches preserve exact RGB and the active Hex representation");
}

#[test]
fn square_and_hue_drag_edit_opaque_rgb() {
    let ctx = crate::fonts::test_context();
    let mut rgb = [255, 0, 0];
    frame(&ctx, vec![], |ui| color_popover(ui, &mut rgb));
    let square = scenario::rect_of("color:sv").unwrap();
    let pos = egui::pos2(square.right(), square.top());
    frame(&ctx, pointer(pos, true), |ui| color_popover(ui, &mut rgb));
    frame(&ctx, vec![egui::Event::PointerMoved(square.center())], |ui| color_popover(ui, &mut rgb));
    frame(&ctx, pointer(square.center(), false), |ui| color_popover(ui, &mut rgb));
    let expected = Color32::from(HsvaGamma { h: 0.0, s: 0.5, v: 0.5, a: 1.0 });
    assert_eq!(rgb, [expected.r(), expected.g(), expected.b()]);
    let hue = scenario::rect_of("color:hue").unwrap();
    frame(&ctx, pointer(hue.left_center(), true), |ui| color_popover(ui, &mut rgb));
    let pos = egui::pos2(hue.left() + hue.width() / 3.0, hue.center().y);
    frame(&ctx, vec![egui::Event::PointerMoved(pos)], |ui| color_popover(ui, &mut rgb));
    frame(&ctx, pointer(pos, false), |ui| color_popover(ui, &mut rgb));
    let expected = Color32::from(HsvaGamma { h: 1.0 / 3.0, s: 0.5, v: 0.5, a: 1.0 });
    assert_eq!(rgb, [expected.r(), expected.g(), expected.b()]);
}

#[test]
fn popover_opens_and_outside_click_closes() {
    let ctx = crate::fonts::test_context();
    let mut rgb = [100, 120, 140];
    frame(&ctx, vec![], |ui| {
        color_field(ui, &mut rgb);
    });
    let at = scenario::handle_at("dialog:color:").unwrap();
    for pressed in [true, false] {
        frame(&ctx, pointer(at, pressed), |ui| {
            color_field(ui, &mut rgb);
        });
    }
    frame(&ctx, vec![], |ui| {
        color_field(ui, &mut rgb);
    });
    assert!(scenario::rect_of("color:popover").is_some());
    for pressed in [true, false] {
        frame(&ctx, pointer(egui::pos2(315.0, 395.0), pressed), |ui| {
            color_field(ui, &mut rgb);
        });
    }
    frame(&ctx, vec![], |ui| {
        color_field(ui, &mut rgb);
    });
    assert!(scenario::rect_of("color:popover").is_none());
}

#[test]
fn channel_edits_use_gamma_bytes_or_linear_values_and_copy_in_the_active_format() {
    use crate::ui_kit::test_support::key;
    for linear in [false, true] {
        let ctx = crate::fonts::test_context();
        let mut rgb = [10, 20, 30];
        frame(&ctx, vec![], |ui| color_popover(ui, &mut rgb));
        if linear {
            click(&ctx, "color:format:F", &mut rgb);
        }
        let at = scenario::handle_at("color:channel:R").unwrap();
        // DragValue switches to its text editor on a click; Ctrl/Cmd+A replaces the channel.
        for pressed in [true, false] {
            frame(&ctx, pointer(at, pressed), |ui| color_popover(ui, &mut rgb));
        }
        let modifiers = egui::Modifiers { command: true, ctrl: true, ..Default::default() };
        frame(
            &ctx,
            vec![
                egui::Event::Key { key: egui::Key::A, physical_key: None, pressed: true, repeat: false, modifiers },
                egui::Event::Text(if linear { "0.5" } else { "128" }.into()),
            ],
            |ui| color_popover(ui, &mut rgb),
        );
        frame(&ctx, key(egui::Key::Enter), |ui| color_popover(ui, &mut rgb));
        assert_eq!(rgb, if linear { [188, 20, 30] } else { [128, 20, 30] });
        let [r, g, b] = rgb;
        let [lr, lg, lb, _] = egui::Rgba::from(Color32::from_rgb(r, g, b)).to_array();
        let want = if linear { format!("{lr:.03}, {lg:.03}, {lb:.03}") } else { format!("{r}, {g}, {b}") };
        assert_eq!(copy(&ctx, &mut rgb), Some(want));
    }
}

#[test]
fn changing_the_hue_of_grey_still_reports_a_manual_colour_edit() {
    let ctx = crate::fonts::test_context();
    let mut rgb = [128, 128, 128];
    frame(&ctx, vec![], |ui| {
        color_field(ui, &mut rgb);
    });
    let at = scenario::handle_at("dialog:color:").unwrap();
    for pressed in [true, false] {
        frame(&ctx, pointer(at, pressed), |ui| {
            color_field(ui, &mut rgb);
        });
    }
    frame(&ctx, vec![], |ui| {
        color_field(ui, &mut rgb);
    });
    let hue = scenario::rect_of("color:hue").unwrap().center();
    frame(&ctx, pointer(hue, true), |ui| {
        color_field(ui, &mut rgb);
    });
    let mut changed = false;
    frame(&ctx, pointer(hue, false), |ui| {
        changed |= color_field(ui, &mut rgb).changed();
    });
    assert!(changed, "a hue edit clears the panel's named look even at zero saturation");
    assert_eq!(rgb, [128, 128, 128]);
}

/// Rasterise the actual tessellated colour geometry at a physical pixel centre. All three
/// controls use the white atlas texel, so interpolated vertex colour is the fragment colour.
fn pixel(primitives: &[egui::ClippedPrimitive], at: egui::Pos2) -> [f32; 4] {
    let mut pixel = [0.0; 4];
    let cross = |a: egui::Vec2, b: egui::Vec2| a.x * b.y - a.y * b.x;
    for clipped in primitives {
        if !clipped.clip_rect.contains(at) {
            continue;
        }
        let egui::epaint::Primitive::Mesh(mesh) = &clipped.primitive else { continue };
        for triangle in mesh.indices.as_chunks::<3>().0 {
            let [a, b, c] = triangle.map(|i| mesh.vertices[i as usize]);
            let area = cross(b.pos - a.pos, c.pos - a.pos);
            if area.abs() < 1e-6 {
                continue;
            }
            let weights = [cross(b.pos - at, c.pos - at) / area, cross(c.pos - at, a.pos - at) / area, cross(a.pos - at, b.pos - at) / area];
            if weights.iter().any(|w| *w < 0.0) {
                continue;
            }
            let mut src = [0.0; 4];
            for (vertex, weight) in [a, b, c].iter().zip(weights) {
                for (channel, value) in src.iter_mut().zip(vertex.color.to_array()) {
                    *channel += f32::from(value) / 255.0 * weight;
                }
            }
            for (dest, value) in pixel.iter_mut().zip(src) {
                *dest = value + *dest * (1.0 - src[3]);
            }
        }
    }
    pixel
}

#[test]
fn rendered_square_hue_and_current_strip_leave_rounded_corner_pixels_clear() {
    let previous = crate::theme::is_dark();
    for dark in [false, true] {
        crate::theme::set_dark(dark);
        for ppp in [1.0, 2.0] {
            let ctx = crate::fonts::test_context();
            ctx.set_pixels_per_point(ppp);
            let mut output = ctx.run_ui(
                egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(320.0, 500.0))), ..Default::default() },
                |ui| {
                    ui.set_width(280.0);
                    crate::ui_kit::scope(ui, |ui| color_popover(ui, &mut [80, 150, 90]));
                },
            );
            output.textures_delta.clear();
            let primitives = ctx.tessellate(output.shapes, output.pixels_per_point);
            for key in ["color:sv", "color:hue", "color:current"] {
                let rect = scenario::rect_of(key).unwrap();
                // Sample the physical pixel just inside each bounding-box corner. The
                // rounded outline and gradient must both leave this pixel transparent.
                for corner in [rect.left_top(), rect.right_top(), rect.left_bottom(), rect.right_bottom()] {
                    let delta = rect.center() - corner;
                    let inward = vec2(delta.x.signum(), delta.y.signum());
                    let at = corner + inward * (0.5 / ppp);
                    assert!(pixel(&primitives, at)[3] < 0.01, "{key} corner {at:?}, dark={dark}, ppp={ppp}");
                }
                assert!(pixel(&primitives, rect.center())[3] > 0.99, "{key} still paints its interior");
            }
        }
    }
    crate::theme::set_dark(previous);
}

/// A taller frame than the shared one: the popover's recent row sits below 400 points.
fn tall_in(ctx: &egui::Context, events: Vec<egui::Event>, id: &str, rgb: &mut [u8; 3]) -> bool {
    let mut changed = false;
    scenario::clear_handles();
    let _ = crate::test_support::frame(
        ctx,
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(320.0, 600.0))),
            events,
            focused: true,
            ..Default::default()
        },
        |ui| {
            ui.set_width(280.0);
            ui.push_id(id, |ui| crate::ui_kit::scope(ui, |ui| changed |= color_popover(ui, rgb)));
        },
    );
    changed
}

fn tall(ctx: &egui::Context, events: Vec<egui::Event>, rgb: &mut [u8; 3]) -> bool {
    tall_in(ctx, events, "picker", rgb)
}

fn tall_click(ctx: &egui::Context, key: &str, rgb: &mut [u8; 3]) -> bool {
    let at = scenario::handle_at(key).unwrap_or_else(|| panic!("missing {key}"));
    tall(ctx, pointer(at, true), rgb) | tall(ctx, pointer(at, false), rgb)
}

fn tall_type(ctx: &egui::Context, field: &str, text: &str, rgb: &mut [u8; 3]) {
    tall_click(ctx, &format!("color:channel:{field}"), rgb);
    let modifiers = egui::Modifiers { command: true, ctrl: true, ..Default::default() };
    tall(
        ctx,
        vec![egui::Event::Key { key: egui::Key::A, physical_key: None, pressed: true, repeat: false, modifiers }, egui::Event::Text(text.into())],
        rgb,
    );
    tall(ctx, crate::ui_kit::test_support::key(egui::Key::Enter), rgb);
}

fn hsv_bytes(h: f32, s: f32, v: f32) -> [u8; 3] {
    let c = Color32::from(HsvaGamma { h, s, v, a: 1.0 });
    [c.r(), c.g(), c.b()]
}

#[test]
fn step_buttons_nudge_rgb_and_hsv_channels_by_one() {
    let ctx = crate::fonts::test_context();
    let mut rgb = [10, 20, 30];
    tall(&ctx, vec![], &mut rgb);
    assert!(tall_click(&ctx, "color:spin:R:up", &mut rgb));
    assert_eq!(rgb, [11, 20, 30]);
    assert!(tall_click(&ctx, "color:spin:B:down", &mut rgb));
    assert_eq!(rgb, [11, 20, 29]);
    // Limits hold: 0 does not step below 0.
    let mut rgb = [0, 0, 0];
    tall(&ctx, vec![], &mut rgb);
    tall_click(&ctx, "color:spin:G:down", &mut rgb);
    assert_eq!(rgb, [0, 0, 0]);
    // H steps one degree, V one percent, on the same gamma HSV as the square.
    let mut rgb = [255, 0, 0];
    tall(&ctx, vec![], &mut rgb);
    tall_click(&ctx, "color:spin:H:up", &mut rgb);
    assert_eq!(rgb, hsv_bytes(1.0 / 360.0, 1.0, 1.0));
    tall_click(&ctx, "color:spin:V:down", &mut rgb);
    assert_eq!(rgb, hsv_bytes(1.0 / 360.0, 1.0, 0.99));
}

#[test]
fn typed_hsv_values_set_the_colour_within_their_ranges() {
    let ctx = crate::fonts::test_context();
    let mut rgb = [255, 0, 0];
    tall(&ctx, vec![], &mut rgb);
    tall_type(&ctx, "H", "120", &mut rgb);
    tall_type(&ctx, "S", "50", &mut rgb);
    assert_eq!(rgb, hsv_bytes(1.0 / 3.0, 0.5, 1.0));
    // A hostile typed value is bounded by the field's range.
    tall_type(&ctx, "V", "1e300", &mut rgb);
    assert_eq!(rgb, hsv_bytes(1.0 / 3.0, 0.5, 1.0));
}

#[test]
fn finished_edits_join_one_recent_list_that_every_picker_offers() {
    let ctx = crate::fonts::test_context();
    let mut rgb = [10, 20, 30];
    tall(&ctx, vec![], &mut rgb);
    assert!(recent_colors(&ctx).is_empty(), "opening a picker records nothing");
    // A drag across the square records only its final colour, once released.
    let square = scenario::rect_of("color:sv").unwrap();
    let end = square.left_top() + vec2(10.0, 10.0);
    tall(&ctx, pointer(square.center(), true), &mut rgb);
    tall(&ctx, vec![egui::Event::PointerMoved(end)], &mut rgb);
    assert!(recent_colors(&ctx).is_empty(), "nothing is recorded mid-drag");
    tall(&ctx, pointer(end, false), &mut rgb);
    assert_eq!(recent_colors(&ctx), vec![rgb]);
    // Fine-tuning in the same opening replaces that entry instead of adding one per step.
    tall_click(&ctx, "color:spin:R:up", &mut rgb);
    tall_click(&ctx, "color:spin:R:up", &mut rgb);
    assert_eq!(recent_colors(&ctx), vec![rgb]);
    let tuned = rgb;
    // Closed (a pass without the popover) and opened again: the next edit is a new entry.
    crate::ui_kit::test_support::frame(&ctx, vec![], |_| {});
    tall(&ctx, vec![], &mut rgb);
    tall_click(&ctx, "color:spin:G:up", &mut rgb);
    assert_eq!(recent_colors(&ctx), vec![rgb, tuned]);
    let second = rgb;
    // Another picker (another colour field of the app) shows the same list and takes from it.
    let mut other = [200, 200, 200];
    tall_in(&ctx, vec![], "another_picker", &mut other);
    assert!(scenario::handle_at("color:recent:2").is_none(), "empty slots are not buttons");
    let slot = scenario::handle_at("color:recent:1").unwrap();
    tall_in(&ctx, pointer(slot, true), "another_picker", &mut other);
    assert!(tall_in(&ctx, pointer(slot, false), "another_picker", &mut other));
    assert_eq!(other, tuned);
    assert_eq!(recent_colors(&ctx), vec![tuned, second], "a picked recent colour moves to the front");
    // Tuning a colour taken from the list adds a new entry and keeps the one it started from.
    let up = scenario::handle_at("color:spin:B:up").unwrap();
    tall_in(&ctx, pointer(up, true), "another_picker", &mut other);
    tall_in(&ctx, pointer(up, false), "another_picker", &mut other);
    assert_eq!(recent_colors(&ctx), vec![other, tuned, second]);
}

#[test]
fn recent_list_is_capped_unique_and_synced_with_the_saved_one() {
    let ctx = crate::fonts::test_context();
    for i in 0..40u8 {
        remember_color(&ctx, [i, 0, 0], None);
    }
    remember_color(&ctx, [35, 0, 0], None);
    let recent = recent_colors(&ctx);
    assert_eq!(recent.len(), MAX_RECENT_COLORS);
    assert_eq!(recent.first(), Some(&[35, 0, 0]));
    assert_eq!(recent.iter().filter(|c| **c == [35, 0, 0]).count(), 1);
    // Picks flow to the saved list once…
    let mut saved = Vec::new();
    crate::recent_colors::share(&ctx, &mut saved);
    assert_eq!(saved, recent);
    // …and a list loaded from preferences (even an oversized one) is what the pickers show.
    let mut loaded: Vec<[u8; 3]> = (0..100u8).map(|i| [0, i, 0]).collect();
    crate::recent_colors::share(&ctx, &mut loaded);
    assert_eq!(loaded.len(), MAX_RECENT_COLORS);
    assert_eq!(recent_colors(&ctx), loaded);
}

#[test]
fn recent_colours_are_saved_with_the_preferences_and_hostile_entries_dropped() {
    use serde_json::json;
    let mut app = crate::SolveApp::new(solidcraft_engine::Session::default(), Default::default());
    let mut list = vec![json!([1, 2, 3]), json!([300, 0, 0]), json!("red"), json!([1, 2]), json!(null)];
    list.extend((0..50).map(|i| json!([i, i, i])));
    app.load_prefs(&json!({ "recent_colors": list }).to_string());
    assert_eq!(app.ui.recent_colors.len(), MAX_RECENT_COLORS);
    assert_eq!(app.ui.recent_colors.first(), Some(&[1, 2, 3]));
    let ctx = crate::fonts::test_context();
    crate::recent_colors::share(&ctx, &mut app.ui.recent_colors);
    remember_color(&ctx, [9, 8, 7], None);
    crate::recent_colors::share(&ctx, &mut app.ui.recent_colors);
    let mut restored = crate::SolveApp::new(solidcraft_engine::Session::default(), Default::default());
    restored.load_prefs(&app.prefs());
    assert_eq!(restored.ui.recent_colors.first(), Some(&[9, 8, 7]));
    assert_eq!(restored.ui.recent_colors.get(1), Some(&[1, 2, 3]));
    assert_eq!(restored.ui.recent_colors.len(), MAX_RECENT_COLORS);
}
