// SolidCraft: a TextEdit centred vertically must put its capitals in the middle of the field,
// not the middle of the font's row box (SolidCraft #22642).
use egui::{Align2, Context, Margin, RawInput, Rect, TextEdit, epaint::Shape, pos2, vec2};

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
    assert!((ink - field_centre).abs() <= 0.6, "ink centre {ink} is {} pt off the field centre {field_centre}", ink - field_centre,);
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
        (ink - field.center().y).abs() <= 0.6,
        "hint ink centre {ink} is {} pt off the field centre {}",
        ink - field.center().y,
        field.center().y,
    );
}
