use super::*;
use crate::test_support::*;

/// The share of a turn an arc of `points` sweeps clockwise (y down) round `centre`.
fn swept(points: &[Pos2], centre: Pos2) -> f32 {
    let turn = std::f32::consts::TAU;
    points.windows(2).map(|w| ((w[1] - centre).angle() - (w[0] - centre).angle()).rem_euclid(turn)).sum::<f32>() / turn
}

/// The buttons group by purpose and wrap as whole groups: the main action first, then the
/// alternative, then the dismissals; no button stands alone on a row while its group-mate fits.
#[test]
fn buttons_group_by_purpose_and_wrap_as_groups() {
    use Action::{Dismiss, Primary, Secondary};
    let four = [(110.0, Primary), (220.0, Secondary), (60.0, Dismiss), (140.0, Dismiss)];
    let three = [(110.0, Primary), (60.0, Dismiss), (140.0, Dismiss)];
    for card in [260.0_f32, 360.0, 460.0, 560.0, 700.0, 1000.0] {
        let row_width = (card - 40.0).min(440.0);
        for buttons in [&four[..], &three[..]] {
            let spots = action_spots(buttons, row_width, 8.0, 28.0);
            let widest = buttons.iter().map(|b| b.0).fold(0.0, f32::max);
            for (i, spot) in spots.iter().enumerate() {
                assert!(spot.left() >= 0.0 && spot.right() <= row_width.max(widest) + 0.01, "card {card}: {spot:?}");
                for other in &spots[i + 1..] {
                    assert!(!spot.intersects(*other), "card {card}: {spot:?} overlaps {other:?}");
                }
            }
            assert!(spots.iter().all(|s| s.top() > spots[0].top() || s.left() >= spots[0].left()), "card {card}: primary first");
            let mut rows: std::collections::BTreeMap<i32, usize> = Default::default();
            for spot in &spots {
                *rows.entry(spot.top() as i32).or_default() += 1;
            }
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

/// In the corner the buttons sit as in the dialogs: right-aligned, dismissals first and the main
/// action right-most; when they wrap, the row with the main action is the bottom one.
#[test]
fn corner_buttons_follow_the_dialogs_and_keep_the_main_action_bottom_right() {
    use Action::{Dismiss, Primary, Secondary};
    let row_width = WIDTH - 40.0;
    let three = [(90.0, Primary), (110.0, Secondary), (50.0, Dismiss)];
    let spots = action_spots_trailing(&three, row_width, 8.0, 28.0);
    assert!(spots.iter().all(|s| s.top() == 0.0), "one row: {spots:?}");
    assert!((spots[0].right() - row_width).abs() < 0.01, "the main action is right-most: {spots:?}");
    assert!(spots[2].right() < spots[1].left() && spots[1].right() < spots[0].left(), "dismiss, alternative, main: {spots:?}");
    let four = [(90.0, Primary), (190.0, Secondary), (50.0, Dismiss), (120.0, Dismiss)];
    let spots = action_spots_trailing(&four, row_width, 8.0, 28.0);
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
    for bad in [f32::NAN, f32::INFINITY, -5.0, 0.0] {
        assert_eq!(action_spots_trailing(&four, bad, 8.0, 28.0).len(), 4);
    }
}

/// The countdown ring around the × is the share of the time left: whole when it starts, half at
/// half time, gone when it is over, and whole for times no clock gives.
#[test]
fn the_countdown_ring_follows_the_time_left() {
    assert_eq!(countdown_left(10.0, 10.0, 6.0), 1.0);
    assert!((countdown_left(10.0, 13.0, 6.0) - 0.5).abs() < 1e-6);
    assert!((countdown_left(0.0, 1.5, 6.0) - 0.75).abs() < 1e-6);
    assert_eq!(countdown_left(10.0, 16.0, 6.0), 0.0);
    assert_eq!(countdown_left(10.0, 5.0, 6.0), 1.0, "a start in the future");
    for (since, now, lifetime) in [(f64::NAN, 1.0, 6.0), (0.0, f64::INFINITY, f64::INFINITY), (0.0, 1.0, 0.0), (0.0, 0.0, 0.0)] {
        let left = countdown_left(since, now, lifetime);
        assert!((0.0..=1.0).contains(&left), "{since} {now} {lifetime}: {left}");
    }
    let centre = pos2(100.0, 50.0);
    for left in [1.0_f32, 0.75, 0.5, 0.25, 0.01] {
        let arc = countdown_arc(centre, 10.0, left);
        assert!(arc.len() >= 2, "{left}");
        assert!(arc.iter().all(|p| ((*p - centre).length() - 10.0).abs() < 1e-3), "{left}: on the ring");
        assert!((arc[0] - pos2(100.0, 40.0)).length() < 1e-3, "starts at twelve o'clock");
        assert!((swept(&arc, centre) - left).abs() < 1e-3, "{left}: sweeps {}", swept(&arc, centre));
    }
    for bad in [0.0_f32, -1.0, f32::NAN] {
        assert!(countdown_arc(centre, 10.0, bad).is_empty(), "{bad}");
    }
    assert!(countdown_arc(centre, f32::NAN, 0.5).is_empty() && countdown_arc(centre, 0.0, 0.5).is_empty());
}

/// The card sits at the bottom of the free part, inside it: centred and as wide as its text, or in
/// the corner `WIDTH` wide against the right; with a × it publishes the ×'s handle. A free part with
/// no room, or none a screen has, draws nothing.
#[test]
fn the_card_sits_at_the_bottom_of_the_free_part() {
    install_catalog();
    let ctx = context();
    let free = Rect::from_min_size(pos2(50.0, 40.0), vec2(800.0, 500.0));
    let mut shown = Vec::new();
    frame(&ctx, Vec::new(), |ui| {
        for place in [Place::Centre, Place::Corner] {
            let extras = Extras { close: Some("toast:test:close"), ..Extras::default() };
            shown.push(show(ui, free, place, "A short message".into(), Lead::Dot(true), &[("Undo", Action::Secondary)], extras));
        }
        for bad in
            [Rect::NOTHING, Rect::from_min_size(pos2(0.0, 0.0), vec2(30.0, 30.0)), Rect::from_min_size(pos2(f32::NAN, 0.0), vec2(800.0, 500.0))]
        {
            assert!(show(ui, bad, Place::Centre, "Message".into(), Lead::Spinner, &[], Extras::default()).is_none(), "{bad:?}");
        }
    });
    let (centre, corner) = match shown.as_slice() {
        [Some(a), Some(b)] => (a, b),
        _ => panic!("both toasts drawn"),
    };
    for toast in [centre, corner] {
        assert!(free.contains_rect(toast.rect), "{:?} inside {free:?}", toast.rect);
        assert!((toast.rect.bottom() - free.bottom()).abs() < 0.5, "at the bottom: {:?}", toast.rect);
        assert_eq!(toast.buttons.len(), 1);
        assert!(toast.close.is_some_and(|x| toast.rect.contains_rect(x)), "the × is on the card");
    }
    assert!((centre.rect.center().x - free.center().x).abs() < 0.5, "centred: {:?}", centre.rect);
    assert!((corner.rect.width() - WIDTH).abs() < 0.5 && (corner.rect.right() - free.right()).abs() < 0.5, "in the corner: {:?}", corner.rect);
    assert!(solidcraft_ui_registry::handle_at("toast:test:close").is_some(), "the × publishes its handle");
}
