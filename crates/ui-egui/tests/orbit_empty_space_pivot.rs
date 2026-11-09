use serde_json::json;
use solidcraft_ui_egui::scenario::Harness;

fn on_screen(h: &mut Harness, w: [f64; 3]) -> [f64; 2] {
    let r = h.call("ui.at", json!({ "world": w }));
    [r["result"][0].as_f64().unwrap(), r["result"][1].as_f64().unwrap()]
}

/// #2616: with Orbit around → Point under the cursor, an orbit begun over empty space turns
/// about the middle of the part in view. It turned about the middle of everything shown, which
/// for a part far from the origin and anything at the origin (a small body, a sketch) is far off
/// screen: the part flew out of view.
#[test]
fn an_orbit_begun_over_empty_space_keeps_a_far_off_part_in_view() {
    for at_origin in ["nothing", "box", "sketch"] {
        let mut h = Harness::new();
        h.app.preferences.orbit_around = solidcraft_ui_egui::prefs::OrbitAround::Cursor;
        h.app.run("solid.box", json!({"length":40,"width":30,"height":20,"corner":[50000,50000,0]})).unwrap();
        h.call("ui.view", json!({"view":"iso"}));
        h.call("ui.view", json!({"view":"fit"}));
        match at_origin {
            "box" => {
                h.app.run("solid.box", json!({"length":10,"width":10,"height":10})).unwrap();
            }
            "sketch" => {
                h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
                h.app.run("sketch.circle.center", json!({"center":[0,0],"radius":5})).unwrap();
                h.app.run("sketch.finish", json!({})).unwrap();
            }
            _ => {}
        }
        h.frames(3);
        let centre = [50020.0, 50015.0, 10.0];
        let before = on_screen(&mut h, centre);
        let rect = h.app.viewport.rect.unwrap();
        // Shift + middle drag from the window's empty corner.
        let a = rect.left_top() + egui::vec2(40.0, 40.0);
        let shift = egui::Modifiers { shift: true, ..Default::default() };
        let send = |h: &mut Harness, e: egui::Event| {
            h.app.synthetic.push(e);
            h.frame();
        };
        send(&mut h, egui::Event::ModifiersChanged(egui::Modifiers::NONE));
        send(&mut h, egui::Event::PointerMoved(a));
        send(&mut h, egui::Event::PointerButton { pos: a, button: egui::PointerButton::Middle, pressed: true, modifiers: shift });
        for k in 1..=18 {
            send(&mut h, egui::Event::PointerMoved(a + egui::vec2(10.0 * k as f32, 0.0)));
        }
        let turned = h.app.cam;
        send(
            &mut h,
            egui::Event::PointerButton { pos: a + egui::vec2(180.0, 0.0), button: egui::PointerButton::Middle, pressed: false, modifiers: shift },
        );
        let after = on_screen(&mut h, centre);
        assert!(turned.yaw.is_finite(), "{at_origin}");
        assert!(
            (after[0] - before[0]).abs() < 2.0 && (after[1] - before[1]).abs() < 2.0,
            "{at_origin} at the origin: the part's middle went from {before:?} to {after:?}"
        );
    }
}
