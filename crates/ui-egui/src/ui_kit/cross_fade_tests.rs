use super::*;
use egui::{Color32, Pos2, Rect, vec2};

const OLD: Color32 = Color32::from_rgb(200, 40, 40);
const NEW: Color32 = Color32::from_rgb(40, 40, 200);

fn region() -> Rect {
    Rect::from_min_size(Pos2::ZERO, vec2(200.0, 100.0))
}

/// One pass at `time` seconds painting the content for `key`: key 0 is an OLD square at the left,
/// any other key a NEW square at the right. Returns the alpha of each colour's square as painted
/// (None when absent) and whether another pass was asked for.
fn pass(ctx: &egui::Context, fade: CrossFade, time: f64, key: u32) -> (Option<u8>, Option<u8>, bool) {
    let input = egui::RawInput { time: Some(time), screen_rect: Some(region()), ..Default::default() };
    let output = ctx.run_ui(input, |ui| {
        fade.show(ui, "content", key, region(), |ui| {
            let (rect, color) = if key == 0 {
                (Rect::from_min_size(Pos2::new(10.0, 10.0), vec2(40.0, 40.0)), OLD)
            } else {
                (Rect::from_min_size(Pos2::new(110.0, 10.0), vec2(40.0, 40.0)), NEW)
            };
            ui.painter().rect_filled(rect, 0.0, color);
        });
    });
    // The squares are told apart by where they are: a faded fill is a darker premultiplied colour.
    let alpha = |left: f32| {
        output.shapes.iter().find_map(|c| match &c.shape {
            Shape::Rect(r) if r.rect.left() == left => Some(r.fill.a()),
            _ => None,
        })
    };
    let repaint = output.viewport_output.get(&egui::ViewportId::ROOT).is_some_and(|v| v.repaint_delay.is_zero());
    (alpha(10.0), alpha(110.0), repaint)
}

#[test]
fn easing_curves_run_from_zero_to_one_and_never_go_back() {
    for easing in [Easing::Linear, Easing::Decelerate, Easing::Accelerate, Easing::Standard, Easing::CubicBezier(0.25, 0.1, 0.25, 1.0)] {
        assert!(easing.ease(0.0).abs() < 1e-4, "{easing:?} starts at 0");
        assert!((easing.ease(1.0) - 1.0).abs() < 1e-4, "{easing:?} ends at 1");
        let mut last = 0.0;
        for i in 0..=100 {
            let v = easing.ease(i as f32 / 100.0);
            assert!(v >= last - 1e-4, "{easing:?} goes back at {i}%");
            last = v;
        }
    }
    assert!(Easing::Decelerate.ease(0.3) > 0.5, "arriving content is mostly in early");
    assert!(Easing::Accelerate.ease(0.3) < 0.3, "accelerate starts slowly");
    assert!((Easing::Standard.ease(0.5) - 0.5).abs() < 0.02, "standard is symmetric");
}

#[test]
fn hostile_progress_and_curves_stay_in_range() {
    for t in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -3.0, 7.0] {
        for easing in [Easing::Linear, Easing::Decelerate, Easing::CubicBezier(f32::NAN, 2.0, -5.0, f32::INFINITY)] {
            let v = easing.ease(t);
            assert!(v.is_finite(), "{easing:?} at {t} gives {v}");
        }
    }
    assert_eq!(Easing::Standard.ease(f32::NAN), 1.0, "not a number counts as finished");
    for duration in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        let fade = CrossFade::new(duration);
        assert!(!fade.animates(), "{duration} does not animate");
        assert_eq!(fade.opacities(0.0, 1.0), (1.0, 0.0), "{duration} swaps at once");
    }
    let (incoming, outgoing) = CrossFade::default().out_share(f32::NAN).opacities(0.05, f32::NAN);
    assert!((0.0..=1.0).contains(&incoming) && (0.0..=1.0).contains(&outgoing));
}

#[test]
fn opacities_cross_over_and_the_old_content_leaves_first() {
    let fade = CrossFade::default();
    assert_eq!(fade.opacities(0.0, 1.0), (0.0, 1.0), "the change starts with only the old content");
    let (incoming, outgoing) = fade.opacities(fade.duration * fade.out_share, 1.0);
    assert!(outgoing.abs() < 1e-4, "the old content is gone after its share");
    assert!(incoming > 0.2 && incoming < 0.8, "the new one is half way in: {incoming}");
    assert_eq!(fade.opacities(fade.duration, 1.0), (1.0, 0.0));
    let (_, outgoing) = fade.opacities(0.0, 0.4);
    assert!((outgoing - 0.4).abs() < 1e-4, "an interrupted change leaves from where it was");
}

#[test]
fn unchanged_content_is_painted_once_at_full_opacity() {
    let ctx = egui::Context::default();
    let fade = CrossFade::default();
    for time in [0.0, 0.1, 0.5] {
        let (old, new, _) = pass(&ctx, fade, time, 0);
        assert_eq!((old, new), (Some(255), None), "at {time}s");
    }
    assert!(!pass(&ctx, fade, 1.0, 0).2, "still content asks for no frames");
}

#[test]
fn a_change_fades_the_old_content_out_and_the_new_one_in() {
    let ctx = egui::Context::default();
    let fade = CrossFade::default();
    pass(&ctx, fade, 0.0, 0);
    pass(&ctx, fade, 1.0, 0);
    // The pass that changes: the new content is painted, invisible, under the old one.
    let (old, new, repaint) = pass(&ctx, fade, 2.0, 1);
    assert_eq!((old, new), (Some(255), Some(0)));
    assert!(repaint, "the fade asks for the next frame");
    // Half way through the out share both are partly shown.
    let (old, new, _) = pass(&ctx, fade, 2.0 + f64::from(fade.duration * fade.out_share / 2.0), 1);
    let (old, new) = (old.unwrap_or(0), new.unwrap_or(0));
    assert!(old > 0 && old < 255 && new > 0 && new < 255, "old {old}, new {new}");
    // Done: only the new content, at full opacity, and no more frames.
    let (old, new, _) = pass(&ctx, fade, 2.0 + f64::from(fade.duration) + 0.01, 1);
    assert_eq!((old, new), (None, Some(255)));
    assert!(!pass(&ctx, fade, 2.5, 1).2, "the fade stops asking for frames");
}

#[test]
fn a_zero_duration_swaps_at_once() {
    let ctx = egui::Context::default();
    let fade = CrossFade::new(0.0);
    pass(&ctx, fade, 0.0, 0);
    pass(&ctx, fade, 0.01, 0);
    let (old, new, _) = pass(&ctx, fade, 0.02, 1);
    assert_eq!((old, new), (None, Some(255)));
    assert!(!pass(&ctx, fade, 0.03, 1).2, "nothing to animate");
}

#[test]
fn changing_back_mid_fade_starts_from_what_is_shown() {
    let ctx = egui::Context::default();
    let fade = CrossFade::default();
    pass(&ctx, fade, 0.0, 0);
    pass(&ctx, fade, 1.0, 1);
    let mid = 1.0 + f64::from(fade.duration) * 0.25;
    let (_, shown, _) = pass(&ctx, fade, mid, 1);
    let shown = shown.unwrap_or(0);
    // Back to the first content: what leaves now (the second) starts no brighter than it was.
    let (_, leaving, _) = pass(&ctx, fade, mid, 0);
    let leaving = leaving.unwrap_or(0);
    assert!(leaving <= shown.saturating_add(1), "shown {shown}, leaving from {leaving}");
}

#[test]
fn a_selection_eases_in_and_out_on_the_fade_curves() {
    let ctx = egui::Context::default();
    let fade = CrossFade::default();
    let id = egui::Id::new("tab");
    // Steady frames at 60 a second, as the app draws while something moves.
    let mut frame = 0;
    let mut run = |frames: usize, selected: bool| -> Vec<f32> {
        (0..frames)
            .map(|_| {
                frame += 1;
                let mut value = 0.0;
                let input = egui::RawInput { time: Some(f64::from(frame) / 60.0), ..Default::default() };
                let _ = ctx.run_ui(input, |ui| value = fade.selected(ui.ctx(), id, selected));
                value
            })
            .collect()
    };
    assert!(run(10, false).iter().all(|v| *v == 0.0), "unselected from the start");
    let rising = run(15, true);
    assert!(rising.windows(2).all(|w| w[0] <= w[1]) && rising[0] < 0.5, "rises: {rising:?}");
    assert_eq!(rising.last().copied(), Some(1.0));
    let falling = run(15, false);
    assert!(falling.windows(2).all(|w| w[0] >= w[1]) && falling[0] > 0.5, "falls: {falling:?}");
    // 0.17 s in, the old highlight is nearly gone rather than lingering to the end.
    assert!(falling[9] < 0.2, "{falling:?}");
    assert_eq!(falling.last().copied(), Some(0.0));
    assert_eq!(CrossFade::new(0.0).selected(&ctx, id, true), 1.0, "no motion: selected at once");
}
