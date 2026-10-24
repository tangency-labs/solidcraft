use super::ball::{Ball, DOTS};
use super::*;

fn reading(axis: usize, value: f32) -> [f32; 6] {
    let mut travel = [0.0; 6];
    travel[axis] = value;
    travel
}

/// Where a point on the cap lands on screen for a reading.
fn on_screen(travel: [f32; 6], p: P3) -> [f32; 2] {
    project(Pose::new(travel).place(p))
}

const TOP_CENTRE: P3 = [0.0, 0.0, CAP_HEIGHT];
const FRONT: P3 = [0.0, -CAP_RADIUS, CAP_HEIGHT / 2.0];

#[test]
fn the_cap_moves_the_way_the_reading_says() {
    let rest = on_screen([0.0; 6], TOP_CENTRE);
    // Slid right: right on screen.
    assert!(on_screen(reading(0, 1.0), TOP_CENTRE)[0] > rest[0]);
    // Pressed down: lower on screen; pulled up: higher.
    assert!(on_screen(reading(2, 1.0), TOP_CENTRE)[1] > rest[1]);
    assert!(on_screen(reading(2, -1.0), TOP_CENTRE)[1] < rest[1]);
    // Pulled toward the user: nearer, so lower on screen.
    assert!(Pose::new(reading(1, 1.0)).place(TOP_CENTRE)[1] < 0.0);
}

#[test]
fn tilts_and_twists_turn_the_cap_the_right_way() {
    let axis = |travel| apply(&Pose::new(travel).turn, [0.0, 0.0, 1.0]);
    assert!((axis([0.0; 6])[2] - 1.0).abs() < 1e-6);
    // +Rx: the top leans toward the user (the near edge down); tilted left: to the left.
    assert!(axis(reading(3, 1.0))[1] < -0.1);
    assert!(axis(reading(4, 1.0))[0] < -0.1);
    // Twisted clockwise (from above): the groove on the front swings to the user's left.
    let rest = Pose::new([0.0; 6]).place(FRONT);
    let twisted = Pose::new(reading(5, 1.0)).place(FRONT);
    assert!(twisted[0] < rest[0] - 0.1);
    assert!((twisted[2] - rest[2]).abs() < 1e-6, "a twist alone keeps the cap's height");
}

#[test]
fn hostile_readings_stay_finite() {
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 1e30] {
        let pose = Pose::new([value; 6]);
        assert!(pose.place(TOP_CENTRE).iter().all(|v| v.is_finite()));
    }
}

#[test]
fn the_hull_keeps_only_the_outline() {
    let square = [(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0), (1.0, 1.0), (1.0, 0.0)];
    let hull = hull(square.iter().map(|&(x, y)| Pos2::new(x, y)).collect());
    assert_eq!(hull.len(), 4);
    assert!(!hull.contains(&Pos2::new(1.0, 1.0)));
    assert!(super::hull(Vec::new()).is_empty());
}

#[test]
fn the_ball_rolls_the_way_it_is_pushed() {
    let top_after = |rate: [f32; 3]| {
        let mut ball = Ball::new();
        // A quarter turn, in small steps as frames give it.
        for _ in 0..25 {
            ball.roll([rate[0], rate[1], rate[2], 0.0], 0.01);
        }
        apply(&ball.turn, [0.0, 0.0, 1.0])
    };
    assert!(top_after([1.0, 0.0, 0.0])[1] > 0.9, "rolled away: the top goes away");
    assert!(top_after([0.0, 1.0, 0.0])[0] > 0.9, "rolled right: the top goes right");
    let mut ball = Ball::new();
    for _ in 0..25 {
        ball.roll([0.0, 0.0, 1.0, 0.0], 0.01);
    }
    // Spun clockwise from above: what faced the user turns to their left.
    assert!(apply(&ball.turn, [0.0, -1.0, 0.0])[0] < -0.9);
}

#[test]
fn the_ball_stays_a_rotation() {
    let mut ball = Ball::new();
    for i in 0..20_000 {
        ball.roll([0.7, -0.3, (i % 7) as f32 * 0.2, 0.5], 0.016);
    }
    for (i, a) in ball.turn.iter().enumerate() {
        for (j, b) in ball.turn.iter().enumerate() {
            let expected = if i == j { 1.0 } else { 0.0 };
            assert!((dot(*a, *b) - expected).abs() < 1e-4);
        }
    }
    for value in [f32::NAN, f32::INFINITY, -1e30] {
        ball.roll([value; 4], value);
        assert!(ball.turn.iter().flatten().all(|v| v.is_finite()) && ball.wheel.is_finite());
        assert!((0..DOTS).all(|i| ball.dot(i).iter().all(|v| v.is_finite())));
    }
}

#[test]
fn the_scroll_ring_turns_and_keeps_its_turn() {
    let mut ball = Ball::new();
    for _ in 0..10 {
        ball.roll([0.0, 0.0, 0.0, 1.0], 0.01);
    }
    let turned = ball.wheel;
    assert!((turned - std::f32::consts::TAU * 0.1).abs() < 1e-4);
    // Let go: a scroll ring stays where it was turned to, and the ball doesn't move with it.
    ball.roll([0.0; 4], 0.01);
    assert_eq!(ball.wheel, turned);
    assert!((apply(&ball.turn, [0.0, 0.0, 1.0])[2] - 1.0).abs() < 1e-6);
}

#[test]
fn each_step_starts_at_rest_and_eases_out_to_full() {
    let [out, hold, back, rest] = DEMO_SECONDS;
    assert_eq!(demo_swing(0.0), 0.0);
    assert!((demo_swing(out) - 1.0).abs() < 1e-6);
    assert_eq!(demo_swing(out + hold / 2.0), 1.0);
    assert_eq!(demo_swing(out + hold + back + rest / 2.0), 0.0);
    // Gently: it creeps away from rest rather than jumping, and never goes back on itself on the way out.
    assert!(demo_swing(out * 0.05) < 0.01);
    let samples: Vec<f32> = (0..=100).map(|i| demo_swing(out * i as f32 / 100.0)).collect();
    assert!(samples.windows(2).all(|w| w[1] >= w[0]));
    // Again from rest after a full showing; nothing odd for odd times.
    assert_eq!(demo_swing(out + hold + back + rest), 0.0);
    for seconds in [f32::NAN, f32::INFINITY, -5.0] {
        assert_eq!(demo_swing(seconds), 0.0);
    }
}

#[test]
fn the_corner_picture_goes_where_it_is_told_and_clears_the_bars() {
    use crate::prefs::SpaceMouseGhost::{LeftViewport as Current, LeftWindow as Left, RightViewport as Right};
    let view = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0));
    let size = egui::vec2(160.0, 120.0);
    // Free: lower left for Current and Left, lower right for Right.
    assert_eq!(corner_rect(view, Current, &[]).left_bottom(), egui::pos2(12.0, 588.0));
    assert_eq!(corner_rect(view, Left, &[]).left_bottom(), egui::pos2(12.0, 588.0));
    assert_eq!(corner_rect(view, Right, &[]).right_bottom(), egui::pos2(788.0, 588.0));
    // The History bar along the bottom pushes each one up to clear it.
    let history = egui::Rect::from_min_max(egui::pos2(0.0, 556.0), egui::pos2(800.0, 596.0));
    for position in [Current, Left, Right] {
        let rect = corner_rect(view, position, &[history]);
        assert_eq!(rect.size(), size);
        assert!(!rect.intersects(history) && rect.bottom() <= history.top() - 12.0 + f32::EPSILON, "{position:?}");
    }
    // A bar that does not reach the picture leaves it where it was.
    let elsewhere = egui::Rect::from_min_max(egui::pos2(300.0, 556.0), egui::pos2(500.0, 596.0));
    for position in [Current, Left, Right] {
        assert_eq!(corner_rect(view, position, &[elsewhere]), corner_rect(view, position, &[]));
    }
    // The navigation bar in the middle of a narrow view is not covered either, nor is anything else stacked.
    let narrow = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(420.0, 600.0));
    let navigation = egui::Rect::from_min_max(egui::pos2(100.0, 520.0), egui::pos2(330.0, 560.0));
    let history = egui::Rect::from_min_max(egui::pos2(0.0, 566.0), egui::pos2(420.0, 600.0));
    for position in [Left, Right] {
        let rect = corner_rect(narrow, position, &[history, navigation]);
        assert!(narrow.contains_rect(rect), "{position:?} stays inside the view");
        assert!(!rect.intersects(history) && !rect.intersects(navigation), "{position:?} clears both bars: {rect:?}");
    }
    // A view narrower than the picture keeps it at the left edge rather than outside.
    let tiny = egui::Rect::from_min_size(egui::pos2(50.0, 0.0), egui::vec2(100.0, 400.0));
    assert!(corner_rect(tiny, Right, &[]).left() >= tiny.left());
    // An empty bar is no bar.
    assert_eq!(corner_rect(view, Left, &[egui::Rect::NOTHING]), corner_rect(view, Left, &[]));
}

#[test]
fn stacked_bars_in_a_short_view_do_not_push_the_picture_out_of_it() {
    use crate::prefs::SpaceMouseGhost::{LeftViewport, LeftWindow, RightViewport};
    let view = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(500.0, 220.0));
    let history = egui::Rect::from_min_max(egui::pos2(0.0, 180.0), egui::pos2(500.0, 220.0));
    let navigation = egui::Rect::from_min_max(egui::pos2(0.0, 130.0), egui::pos2(500.0, 170.0));
    let chip = egui::Rect::from_min_max(egui::pos2(0.0, 80.0), egui::pos2(500.0, 120.0));
    for position in [LeftViewport, RightViewport, LeftWindow] {
        let rect = corner_rect(view, position, &[history, navigation, chip]);
        assert!(rect.top() >= view.top() && view.contains_rect(rect), "{position:?}: {rect:?} is outside {view:?}");
    }
}

/// How much one frame of each picture costs the painter: shapes and tessellated vertices. A ceiling
/// each, so a picture cannot quietly grow expensive (the calibration window once ran a core flat out).
#[test]
fn the_pictures_cost_a_bounded_amount_to_draw_per_frame() {
    let ctx = crate::fonts::test_context();
    let calibration = Calibration {
        dead_zone_share: 0.0,
        kind: Kind::ReturnToCenter,
        axes: std::array::from_fn(|source| solidcraft_space_mouse::AxisCalibration {
            source,
            reversed: false,
            rest: 0,
            dead_zone: 0,
            full: [350; 2],
        }),
    };
    let cost = |draw_it: &mut dyn FnMut(&mut egui::Ui)| {
        let mut output = ctx.run_ui(
            egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0))), ..Default::default() },
            |ui| draw_it(ui),
        );
        output.textures_delta.clear();
        let shapes = output.shapes.len();
        let vertices: usize = ctx
            .tessellate(output.shapes, 1.0)
            .iter()
            .filter_map(|p| if let egui::epaint::Primitive::Mesh(m) = &p.primitive { Some(m.vertices.len()) } else { None })
            .sum();
        (shapes, vertices)
    };
    let cap = cost(&mut |ui| live(ui, &calibration, [60, -40, 100, 10, -20, 30]));
    let ball = Calibration { kind: Kind::Trackball, ..calibration };
    let rolling = cost(&mut |ui| live(ui, &ball, [60, -40, 100, 10, -20, 30]));
    let demo_cost = cost(&mut |ui| demo(ui, solidcraft_space_mouse::STEPS[0], Kind::ReturnToCenter));
    eprintln!("PICTURE COST cap (shapes, vertices) {cap:?} trackball {rolling:?} demo {demo_cost:?}");
    for (name, (shapes, vertices)) in [("cap", cap), ("trackball", rolling), ("demo", demo_cost)] {
        assert!(shapes <= CEILING_SHAPES && vertices <= CEILING_VERTICES, "{name}: {shapes} shapes, {vertices} vertices");
    }
}

/// Measured at 45 shapes and 1,374 vertices for the cap, 82 and 2,534 for the trackball.
const CEILING_SHAPES: usize = 150;
const CEILING_VERTICES: usize = 4_000;

#[test]
fn the_demo_repaints_at_20_fps_only_while_it_moves() {
    use std::time::Duration;
    let [out, hold, back, rest] = DEMO_SECONDS;
    assert_eq!(demo_repaint_after(out / 2.0, true), Duration::from_millis(50), "moving out");
    assert_eq!(demo_repaint_after(out + hold / 2.0, true), Duration::from_millis(250), "holding");
    assert_eq!(demo_repaint_after(out + hold + back / 2.0, true), Duration::from_millis(50), "moving back");
    assert_eq!(demo_repaint_after(out + hold + back + rest / 2.0 - 0.1, true), Duration::from_millis(250), "resting");
    assert_eq!(demo_repaint_after(out / 2.0, false), Duration::from_millis(500), "not in use");
}

/// The demo asks for its own pace and nothing shorter, for a ball as for a cap: 50 ms while it moves with the
/// window in use, and 500 ms when the window is not (the ball's own 16 ms request used to win both).
#[test]
fn the_demo_asks_for_its_own_repaint_pace_for_a_ball_and_a_cap() {
    let step = Step { axis: 1, positive: true };
    for kind in [Kind::Trackball, Kind::ReturnToCenter] {
        for (focused, expected) in [(true, std::time::Duration::from_millis(50)), (false, std::time::Duration::from_millis(500))] {
            let ctx = crate::fonts::test_context();
            let mut delay = None;
            for time in [0.0, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6] {
                let mut output = ctx.run_ui(egui::RawInput { time: Some(time), focused, ..Default::default() }, |ui| demo(ui, step, kind));
                output.textures_delta.clear();
                delay = output.viewport_output.get(&egui::ViewportId::ROOT).map(|viewport| viewport.repaint_delay);
            }
            // egui takes a frame's worth off the delay it reports, so allow for that.
            let delay = delay.expect("a repaint delay");
            assert!(
                delay <= expected && delay + std::time::Duration::from_millis(20) >= expected,
                "{kind:?}, focused {focused}: {delay:?}, wanted about {expected:?}"
            );
        }
    }
}
