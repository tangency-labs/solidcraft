// SolidCraft: a TextEdit centred vertically must put its capitals in the middle of the field,
// not the middle of the font's row box (SolidCraft #23381).
use egui::{Align2, Context, FontId, Frame, Margin, RawInput, Rect, TextEdit, epaint::Shape, pos2, vec2};

fn ink_and_field(text: &str, align: Align2) -> (f32, f32) {
    let ctx = Context::default();
    let mut value = text.to_owned();
    let mut field = Rect::ZERO;
    let mut output =
        ctx.run_ui(RawInput { screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 300.0))), ..Default::default() }, |ui| {
            let response = ui.add_sized(vec2(200.0, 40.0), TextEdit::singleline(&mut value).align(align).margin(Margin::ZERO));
            field = response.rect;
        });
    output.textures_delta.clear();
    let ink = output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            Shape::Text(t) => Some(t.pos.y + t.galley.mesh_bounds.center().y),
            _ => None,
        })
        .expect("a painted text shape");
    (ink, field.center().y)
}

#[test]
fn centred_text_edit_puts_capital_ink_in_the_middle_of_the_field() {
    let (ink, field_centre) = ink_and_field("Hamburgevons", Align2::LEFT_CENTER);
    // The row box would land the capitals low; the ink's middle must not.
    assert!((ink - field_centre).abs() <= 0.5, "ink centre {ink} is {} pt off the field centre {field_centre}", ink - field_centre,);
}

#[test]
fn top_aligned_text_edit_is_unchanged() {
    let (ink, field_centre) = ink_and_field("Hamburgevons", Align2::LEFT_TOP);
    assert!(ink < field_centre, "top-aligned ink {ink} should sit above the field centre {field_centre}");
}

#[test]
fn the_ink_stays_put_between_the_field_and_its_neighbours() {
    // Changing what is typed must not move the baseline: the rise comes from a fixed probe,
    // not from the text.
    let (wide, centre) = ink_and_field("0000", Align2::LEFT_CENTER);
    let (narrow, _) = ink_and_field("0", Align2::LEFT_CENTER);
    assert_eq!(wide, narrow, "the baseline moved while typing (field centre {centre})");
}

#[test]
fn a_hinted_empty_text_edit_puts_the_hint_ink_in_the_middle_of_the_field() {
    let ctx = Context::default();
    let mut value = String::new();
    let mut field = Rect::ZERO;
    let mut output = ctx.run_ui(
        RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 300.0))),
            ..Default::default()
        },
        |ui| {
            let response = ui.add_sized(
                vec2(200.0, 40.0),
                TextEdit::singleline(&mut value)
                    .align(Align2::LEFT_CENTER)
                    .margin(Margin::ZERO)
                    .hint_text("Hamburgevons"),
            );
            field = response.rect;
        },
    );
    output.textures_delta.clear();
    let ink = output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            Shape::Text(t) if !t.galley.is_empty() => Some(t.pos.y + t.galley.mesh_bounds.center().y),
            _ => None,
        })
        .expect("a painted hint shape");
    assert!(
        (ink - field.center().y).abs() <= 0.5,
        "hint ink centre {ink} is {} pt off the field centre {}",
        ink - field.center().y,
        field.center().y,
    );
}

/// The preview insertion cursor painted while the pointer moves over the field with a
/// button down must stand at exactly the height of the caret painted in the same frame:
/// both sit on the lifted baseline, not one on the lifted row and the other on the row
/// box (#23381).
#[test]
fn the_preview_cursor_stands_where_the_caret_lands() {
    let ctx = Context::default();
    ctx.style_mut_of(ctx.theme(), |style| {
        style.visuals.text_cursor.blink = false;
        style.visuals.text_cursor.preview = true;
    });
    let mut value = "Hamburgevons".to_owned();
    // A font whose capitals are not already centred in their row box: its lift is
    // non-zero, so the two cursors would separate without the preview sharing it.
    let font = FontId::monospace(15.0);
    // Press on the field, then keep moving with the button down: the caret follows the
    // press and the preview follows the pointer, and both are painted in the last frame.
    let press = pos2(57.0, 20.0);
    let frames: Vec<Vec<egui::Event>> = vec![
        vec![egui::Event::PointerMoved(press)],
        vec![egui::Event::PointerButton {
            pos: press,
            pressed: true,
            button: egui::PointerButton::Primary,
            modifiers: egui::Modifiers::NONE,
        }],
        vec![egui::Event::PointerMoved(pos2(58.0, 20.0))],
        vec![egui::Event::PointerMoved(pos2(60.0, 20.0))],
    ];
    let mut output: Option<egui::FullOutput> = None;
    for (frame, events) in frames.iter().enumerate() {
        if let Some(mut previous) = output.take() {
            previous.textures_delta.clear(); // the font atlas updates of earlier frames
        }
        output = Some(ctx.run_ui(
            RawInput {
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 300.0))),
                events: events.clone(),
                time: Some(frame as f64 / 60.0), // real frames pass time, so a moved pointer has velocity
                focused: true,
                ..Default::default()
            },
            |ui| {
                ui.add_sized(
                    vec2(200.0, 40.0),
                    TextEdit::singleline(&mut value)
                        .align(Align2::LEFT_CENTER)
                        .margin(Margin::ZERO)
                        .frame(Frame::NONE)
                        .font(font.clone()),
                );
            },
        ));
    }
    let mut output = output.expect("at least one frame");
    output.textures_delta.clear();
    let cursors: Vec<f32> = output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            Shape::LineSegment { points, .. } => Some((points[0].y + points[1].y) * 0.5),
            _ => None,
        })
        .collect();
    assert_eq!(
        cursors.len(),
        2,
        "the frame paints the caret and the preview cursor: {cursors:?}"
    );
    let (caret, preview) = (cursors[0], cursors[1]);
    assert!(
        (preview - caret).abs() <= 0.5,
        "the preview cursor centre {preview} is {} pt off the caret centre {caret}",
        preview - caret,
    );
}

/// A prefix shares the field's vertical placement: a centred field lifts it together with
/// its text, and a top-aligned field lifts neither, so an affix never sits on a different
/// baseline than the text beside it (SolidCraft #23381).
#[test]
fn a_prefix_shares_the_field_vertical_placement() {
    let tops = |align: Align2| -> (f32, f32, f32) {
        let ctx = Context::default();
        let mut value = "Hamburgevons".to_owned();
        let font = FontId::monospace(15.0);
        let mut rise = 0.0;
        let mut output = ctx.run_ui(
            RawInput {
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 300.0))),
                ..Default::default()
            },
            |ui| {
                rise = egui::cap_ink_rise(ui.painter(), &font);
                ui.add_sized(
                    vec2(200.0, 40.0),
                    TextEdit::singleline(&mut value)
                        .align(align)
                        .margin(Margin::ZERO)
                        .font(font.clone())
                        .prefix(egui::RichText::new("Q").font(font.clone())),
                );
            },
        );
        output.textures_delta.clear();
        let mut prefix = None;
        let mut text = None;
        for clipped in &output.shapes {
            if let Shape::Text(t) = &clipped.shape {
                if t.galley.job.text == "Q" {
                    prefix = Some(t.pos.y);
                } else {
                    text = Some(t.pos.y);
                }
            }
        }
        (
            prefix.expect("the prefix is painted"),
            text.expect("the value is painted"),
            rise,
        )
    };
    let (prefix, text, rise) = tops(Align2::LEFT_CENTER);
    assert!(rise.abs() >= 0.5, "guard: this font must have a non-zero lift, got {rise}");
    assert!(
        (prefix - text).abs() <= 0.001,
        "in a centred field the prefix row top {prefix} must match the text's {text}"
    );
    let (prefix, text, _) = tops(Align2::LEFT_TOP);
    assert!(
        (prefix - text).abs() <= 0.001,
        "in a top-aligned field the prefix row top {prefix} must match the text's {text}"
    );
}
