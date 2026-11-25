use super::*;

fn button_at(x: f32) -> Ask {
    let rect = Rect::from_min_size(egui::pos2(x, 0.0), egui::vec2(32.0, 32.0));
    Ask { region: Region::Button, rect, at: rect.center(), base: Color32::DARK_GRAY, enabled: 1.0 }
}

#[test]
fn with_no_tint_on_nothing_is_asked_or_answered() {
    let ctx = Context::default();
    assert_eq!(ask(&ctx, button_at(0.0)), None);
    assert!(take_asked(&ctx).is_empty());
    answer(&ctx, &[button_at(0.0)], &[[1.0, 0.0, 0.0, 1.0]]);
    assert_eq!(ask(&ctx, button_at(0.0)), None, "answers are ignored while off");
}

#[test]
fn answers_come_back_for_the_regions_asked_and_off_forgets_them() {
    let ctx = Context::default();
    set_on(&ctx, true);
    assert_eq!(ask(&ctx, button_at(0.0)), None, "no answer before the first evaluation");
    ask(&ctx, button_at(0.0));
    ask(&ctx, button_at(200.0));
    let asked = take_asked(&ctx);
    assert_eq!(asked.len(), 2, "one ask per region a frame");
    answer(&ctx, &asked, &[[1.0, 0.0, 0.0, 0.5], [0.0, f32::NAN, 2.0, 1.0]]);
    set_on(&ctx, true);
    assert_eq!(ask(&ctx, button_at(0.0)), Some([1.0, 0.0, 0.0, 0.5]));
    assert_eq!(ask(&ctx, button_at(200.0)), Some([0.0, 0.0, 1.0, 1.0]), "NaN and out of range are made safe");
    assert_eq!(ask(&ctx, button_at(10.0)), Some([1.0, 0.0, 0.0, 0.5]), "a moved region takes its nearest answer");
    assert_eq!(ask(&ctx, button_at(600.0)), None);
    set_on(&ctx, false);
    set_on(&ctx, true);
    assert_eq!(ask(&ctx, button_at(0.0)), None, "turning off forgets the answers");
}

#[test]
fn the_region_bytes_follow_the_prelude_layout() {
    let mut a = button_at(10.0);
    a.base = Color32::WHITE;
    a.enabled = 0.35;
    let b = a.bytes();
    let f = |i: usize| f32::from_le_bytes([b[i * 4], b[i * 4 + 1], b[i * 4 + 2], b[i * 4 + 3]]);
    assert_eq!([f(0), f(1), f(2), f(3)], [10.0, 0.0, 42.0, 32.0]);
    assert_eq!([f(4), f(5), f(6), f(7)], [1.0, 1.0, 1.0, 1.0]);
    assert_eq!([f(8), f(9)], [26.0, 16.0]);
    assert_eq!(u32::from_le_bytes([b[40], b[41], b[42], b[43]]), Region::Button as u32);
    assert_eq!(f(11), 0.35);
}
