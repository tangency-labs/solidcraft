use super::*;

fn run(mut lift: Lift, up: bool, frames: usize) -> (Lift, f32, f32) {
    let (mut most, mut least) = (lift.height, lift.height);
    for _ in 0..frames {
        lift = lift.step(up, 1.0 / 60.0);
        most = most.max(lift.height);
        least = least.min(lift.height);
    }
    (lift, most, least)
}

#[test]
fn the_lift_overshoots_a_little_settles_and_lands_without_going_through_the_ground() {
    let (up, most, _) = run(Lift::default(), true, 90);
    assert_eq!(up, Lift { height: 1.0, speed: 0.0 }, "it comes to rest, so no more frames are asked for");
    assert!(most > 1.03 && most < 1.2, "{most}");
    let (down, _, least) = run(up, false, 120);
    assert_eq!(down, Lift::default());
    assert!(least >= 0.0, "{least}");
    // A long stall between frames does not throw the spring off.
    let stalled = Lift::default().step(true, 10.0);
    assert!(stalled.height >= 0.0 && stalled.height < 1.2);
}

#[test]
fn a_blur_spreads_the_footprint_without_moving_it() {
    let size = [9, 7];
    let mut mask = vec![0u8; 63];
    mask[3 * 9 + 4] = 255;
    let (cells, grown) = blurred(&mask, size, 1).unwrap();
    assert_eq!(grown, [9 + 2 * BLUR_ROOM, 7 + 2 * BLUR_ROOM]);
    let at = |x: usize, y: usize| cells[y * grown[0] + x];
    let (x, y) = (4 + BLUR_ROOM, 3 + BLUR_ROOM);
    assert!(at(x, y) > 0 && at(x - 1, y) == at(x + 1, y) && at(x, y - 1) == at(x, y + 1));
    assert!(at(x, y) >= at(x + 1, y));
    assert_eq!(at(0, 0), 0);
    assert!(blurred(&mask, [9, 8], 1).is_none(), "a mask of the wrong length");
    assert_eq!(covered(&mask, size), Some(Rect::from_min_max(pos2(4.0 / 9.0, 3.0 / 7.0), pos2(5.0 / 9.0, 4.0 / 7.0))));
    assert_eq!(covered(&[0; 63], size), None);
}

#[test]
fn a_shrunk_picture_keeps_its_shape_and_averages_its_pixels() {
    let mut image = ColorImage::filled([512, 384], Color32::TRANSPARENT);
    for (i, p) in image.pixels.iter_mut().enumerate() {
        if i % 2 == 0 {
            *p = Color32::from_rgba_premultiplied(200, 100, 0, 200);
        }
    }
    let small = shrink(&image, 128);
    assert_eq!(small.size, [128, 96]);
    assert_eq!(small.pixels[0], Color32::from_rgba_premultiplied(100, 50, 0, 100));
    assert_eq!(shrink(&small, 384).size, [128, 96], "never enlarged");
}

#[test]
fn a_preview_draws_with_and_without_a_picture_at_any_size() {
    let ctx = Context::default();
    let shadow = PreviewShadow { mask: vec![255; 16], size: [4, 4], area: Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)) };
    let textures = PreviewTextures::load(&ctx, "test", PreviewImage { picture: ColorImage::filled([8, 6], Color32::WHITE), shadow: Some(shadow) });
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        for (n, size) in [vec2(48.0, 36.0), vec2(300.0, 84.0), vec2(2000.0, 4.0), vec2(0.0, 0.0), vec2(268.0, 150.0)].into_iter().enumerate() {
            let rect = Rect::from_min_size(pos2(10.0, 10.0), size);
            file_preview(ui, Id::unique(n), rect, Some(textures.art()), true, 1.0);
            file_preview(ui, Id::unique(("empty", n)), rect, None, true, 0.4);
        }
    });
    assert!(!output.shapes.is_empty());
    output.textures_delta.clear();
}
