use serde_json::{Value, json};

use crate::Session;

fn run(s: &mut Session, command: &str, params: Value) -> Value {
    s.execute(command, &params).unwrap_or_else(|e| panic!("{command}: {e}"))
}

fn bottom_rounds() -> Session {
    let mut s = Session::default();
    run(&mut s, "parameters.change", json!({"name":"corner_radius", "expression":"2 mm"}));
    run(&mut s, "sketch.create", json!({"plane":"XY", "name":"Rectangle"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0":[0,0], "p1":[25,25]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance":"10 mm", "body_name":"Block"}));
    run(&mut s, "solid.fillet", json!({"edges":[[12.5,0,0],[25,12.5,0],[12.5,25,0],[0,12.5,0]], "radius":"2 mm"}));
    s
}

fn volume(s: &mut Session) -> f64 {
    let value = run(s, "inspect.measure", json!({}));
    assert_eq!(value["body_count"], 1);
    let state = s.model.state();
    let body = &state.body("Block").unwrap().body;
    assert!(body.validity().is_empty(), "{:?}", body.validity());
    value["total"]["volume_mm3"].as_f64().unwrap()
}

#[test]
fn issue1557_fillet_recomputes_and_survives_undo_and_reload() {
    for edges in [json!([[25, 0, 6]]), json!([[25, 0, 6], [24.287266336668306, 0.7127336633316916, 0.48266373325510686]])] {
        let mut s = bottom_rounds();
        let before = volume(&mut s);
        let out = run(&mut s, "solid.fillet", json!({"edges":edges, "radius":"corner_radius"}));
        let id = out["feature"].as_u64().unwrap();
        let after = volume(&mut s);
        assert!(after < before - 1.0);
        assert_eq!(s.doc.features.len(), 4);
        run(&mut s, "edit.undo", json!({}));
        assert!((volume(&mut s) - before).abs() < 1e-6);
        run(&mut s, "edit.redo", json!({}));
        assert!((volume(&mut s) - after).abs() < 1e-6);
        run(&mut s, "parameters.change", json!({"name":"corner_radius", "expression":"1 mm"}));
        let smaller = volume(&mut s);
        assert!(smaller > after && smaller < before);
        assert!(run(&mut s, "timeline.inspect", json!({"feature":id}))["error"].is_null());
        let bytes = crate::io::write_design_container(&s.doc).unwrap();
        let mut reopened = Session::new(crate::io::read_design(&bytes).unwrap());
        assert!((volume(&mut reopened) - smaller).abs() < 1e-6);
        assert_eq!(reopened.doc.features.len(), 4);
    }
}

#[test]
fn issue1557_failed_radius_keeps_the_existing_rounds() {
    let mut s = bottom_rounds();
    let before = s.doc.to_json();
    let v = volume(&mut s);
    let undo = s.undo.len();
    assert!(s.execute("solid.fillet", &json!({"edges":[[25,0,6]], "radius":"1000 mm"})).is_err());
    assert_eq!(s.doc.to_json(), before);
    assert_eq!(s.undo.len(), undo);
    assert!((volume(&mut s) - v).abs() < 1e-6);
}

/// A cylinder of diameter `d` and height `h` with a square notch cut into its top rim (4.8 x 4.8
/// for d = 40, scaled with d), its top face's outer loop rounded with radius `r`. Returns the
/// volume before and after the round.
fn notched_rim_round(d: f64, h: f64, r: f64) -> (f64, f64) {
    let k = d / 40.0;
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"diameter": d, "height": h, "operation": "new", "body_name": "Base"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "NotchSketch1", "offset": h}));
    run(&mut s, "sketch.rectangle.center", json!({"center": [0.0, 20.0 * k], "corner": [2.4 * k, 22.4 * k]}));
    run(&mut s, "sketch.finish", json!({}));
    let notch = json!({"sketch": "NotchSketch1", "distance": 20.0 * k, "direction": "negative", "operation": "cut", "targets": ["Base"]});
    run(&mut s, "solid.extrude", notch);
    let measure = |s: &mut Session| {
        let value = run(s, "inspect.measure", json!({}));
        assert_eq!(value["body_count"], 1, "{value}");
        assert_eq!(value["bodies"][0]["closed"], true, "{value}");
        assert_eq!(value["bodies"][0]["notes"], json!([]), "{value}");
        let state = s.model.state();
        let body = &state.body("Base").unwrap().body;
        assert!(body.validity().is_empty(), "{:?}", body.validity());
        value["total"]["volume_mm3"].as_f64().unwrap()
    };
    let before = measure(&mut s);
    let rim = [-18.7938524157 * k, -6.84040286651 * k, h];
    let edges = json!([rim, [2.4 * k, 18.8 * k, h], [-2.4 * k, 18.8 * k, h], [0.0, 17.6 * k, h]]);
    let out = s.execute("solid.fillet", &json!({"edges": edges, "radius": r, "body": "Base"}));
    if let Err(e) = out {
        panic!("d {d} h {h} r {r}: {e}");
    }
    (before, measure(&mut s))
}

/// #18543: the top loop of a cylinder with a square notch in its rim (the rim, then in and out
/// around the notch) is rounded, at the sizes it was refused at with "the fillet is larger than
/// the faces beside its edges allow; no smaller radius builds either". A convex round of radius r
/// along a loop of length L takes (1 - pi/4) r^2 L; the loop is the rim less the notch's mouth
/// plus the notch's three walls, about 2 pi R + 4.7 k for notch scale k.
/// (A long thin one, d 1 x 1000, is still refused: "the rounds would leave openings".)
#[test]
fn issue18543_the_top_loop_of_a_cylinder_with_a_notch_in_its_rim_is_rounded() {
    for (d, h, r) in [(40.0, 60.0, 1.0), (40.0, 60.0, 0.3), (3.0, 4.5, 0.075), (1.0, 1.5, 0.025), (1200.0, 1800.0, 30.0)] {
        let (before, after) = notched_rim_round(d, h, r);
        let k = d / 40.0;
        let full = (1.0 - std::f64::consts::FRAC_PI_4) * r * r * (std::f64::consts::PI * d + 4.7 * k);
        let taken = before - after;
        assert!(taken > 0.5 * full && taken < 1.2 * full, "d {d} h {h} r {r}: took {taken} of about {full}");
    }
}
