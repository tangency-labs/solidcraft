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

/// Volume of the one body, which must be valid.
fn valid_volume(s: &mut Session, body: &str) -> f64 {
    let state = s.model.state();
    let b = &state.body(body).unwrap().body;
    assert!(b.validity().is_empty(), "{:?}", b.validity());
    run(s, "inspect.measure", json!({"bodies": [body]}))["bodies"][0]["volume_mm3"].as_f64().unwrap()
}

/// A round that builds takes material off and leaves a valid body; one the kernel can't build is
/// refused in the app's own words, never with a bare blend status.
fn rounds_or_says_why(s: &mut Session, body: &str, params: Value) {
    let before = valid_volume(s, body);
    match s.execute("solid.fillet", &params) {
        Ok(_) => {
            let after = valid_volume(s, body);
            assert!(after < before, "{after} vs {before}");
        }
        Err(e) => {
            let e = e.to_string();
            assert!(!e.contains("status 1001") && !e.contains("Fillet Corner Error"), "{e}");
        }
    }
}

/// A 4 mm round on a block's top edge where a 10 mm boss stands across that edge (its axis 1 mm
/// inside it) runs along the edge and ends at the boss wall. Today SMLib's corner fails
/// ("Fillet Corner Error: Corner Geometry Computation Failure", status 1001)
/// ([T-95](https://tickets.solidcraft.io/t/95), #17590).
#[test]
fn a_round_on_an_edge_a_boss_stands_across_ends_at_the_boss() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 70, "width": 40, "height": 25, "corner": [-35, -20, 0], "body_name": "Block"}));
    run(&mut s, "solid.cylinder", json!({"radius": 5, "height": 10, "base": [0, -19, 23], "operation": "join", "targets": ["Block"]}));
    rounds_or_says_why(&mut s, "Block", json!({"edges": [[-25, -20, 25]], "radius": 4}));
}

/// A 6 mm round on the circle where a 120 mm tube stands on its 180 mm flange, picked across
/// from a 22 mm pipe that straddles the tube's wall: with the tangent chain on it is the same
/// round as with it off, since the base circle meets the pipe's at sharp corners and has no
/// tangent neighbour to add. Today the chain makes it "larger than the faces beside its edges
/// allow" at every radius, while `tangent_chain: false` builds
/// ([T-96](https://tickets.solidcraft.io/t/96), #17591).
#[test]
fn a_tube_to_flange_round_beside_a_crossing_pipe_is_the_same_with_the_tangent_chain() {
    let part = |s: &mut Session| {
        run(s, "solid.cylinder", json!({"diameter": 180, "height": 6, "body_name": "Flange"}));
        run(s, "solid.cylinder", json!({"diameter": 120, "height": 80, "operation": "join", "targets": ["Flange"]}));
        run(s, "solid.cylinder", json!({"diameter": 22, "height": 74, "base": [66, 0, 6], "operation": "join", "targets": ["Flange"]}));
    };
    let mut without = Session::default();
    part(&mut without);
    run(&mut without, "solid.fillet", json!({"edges": [[-60, 0, 6]], "radius": 6, "tangent_chain": false}));
    let want = valid_volume(&mut without, "Flange");
    let mut with = Session::default();
    part(&mut with);
    run(&mut with, "solid.fillet", json!({"edges": [[-60, 0, 6]], "radius": 6}));
    let got = valid_volume(&mut with, "Flange");
    assert!((got - want).abs() < 1e-6 * want, "{got} vs {want}");
}

/// A 1.5 mm round on the vertical edge of a shallow (about 176°) outline corner where a pocket's
/// wall starts, so the edge ends on the pocket's floor, runs up to that floor. Without the pocket
/// it builds; with it SMLib's corner fails ("Fillet Corner Error: Unable to Make Topology",
/// status 1001) ([T-103](https://tickets.solidcraft.io/t/103), #17600).
#[test]
fn a_round_on_an_outline_edge_ending_at_a_pocket_floor_builds() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Outline"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [500, 0], [500, 150], [300, 150], [100, 135], [0, 135]], "closed": true}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 40, "body_name": "Plate"}));
    // 15 deep from the top, one wall on x = 300, open through the side at y = 150.
    run(&mut s, "solid.box", json!({"length": 60, "width": 30, "height": 20, "center": [330, 137, 35], "operation": "cut", "targets": ["Plate"]}));
    rounds_or_says_why(&mut s, "Plate", json!({"edges": [[300, 150, 10]], "radius": 1.5}));
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

/// A 2 thick wall, 30 x 2 x 10: past 1 mm its two top edges' rounds overlap on the 2 wide top.
fn wall() -> Session {
    let mut s = Session::default();
    run(&mut s, "parameters.change", json!({"name":"top_radius", "expression":"0.5 mm"}));
    run(&mut s, "sketch.create", json!({"plane":"XY", "name":"Wall"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0":[0,0], "p1":[30,2]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance":"10 mm", "body_name":"Wall"}));
    s
}

/// #17926: a fillet the kernel refuses fails with the kernel call's stable code beside its
/// error, a kernel kind (not one the operations library decided). The same failure, from a new
/// fillet or from an existing one whose parameter changed, has the same code, in the command's
/// error and in the timeline's result; a fillet that builds has none.
#[test]
fn a_refused_fillet_carries_the_kernel_calls_code() {
    let mut s = wall();
    let edges = json!([[15, 0, 10], [15, 2, 10]]);
    let e = s.execute("solid.fillet", &json!({"edges":edges, "radius":"1.5 mm"})).unwrap_err();
    let code = e.error_code().cloned().unwrap_or_else(|| panic!("no code: {e}"));
    assert_ne!(code.kind, 0, "{code}");
    assert_eq!(code.call, "blend", "{code}");

    let out = run(&mut s, "solid.fillet", json!({"edges":edges, "radius":"top_radius"}));
    let id = out["feature"].as_u64().unwrap();
    let inspected = run(&mut s, "timeline.inspect", json!({"feature":id}));
    assert!(inspected["error"].is_null() && inspected.get("error_code").is_none(), "{inspected}");
    s.execute("parameters.change", &json!({"name":"top_radius", "expression":"1.8 mm"})).ok();
    let inspected = run(&mut s, "timeline.inspect", json!({"feature":id}));
    assert!(inspected["error"].is_string(), "{inspected}");
    let timeline_code = &inspected["error_code"];
    assert_eq!(timeline_code["call"], code.call.as_str(), "{inspected}");
    assert_eq!(timeline_code["kind"], code.kind, "{inspected}");
    assert_eq!(timeline_code["operation"], code.operation.as_str(), "{inspected}");
    let doc = run(&mut s, "document.inspect", json!({"timeline":"summary"}));
    let row = doc["timeline"].as_array().unwrap().iter().find(|r| r["id"] == id).cloned().unwrap();
    assert_eq!(&row["error_code"], timeline_code, "{row}");
}

/// #22148: a failure's code says the kernel's class of its error, so a client tells "change the
/// input" from the kernel's fault without a copy of the kind table. A can (a cylinder of radius
/// 10, open at its top) shelled thicker than its radius is refused by the kernel's offset (its
/// wall's offset collapses or folds, kinds 200 to 204, every one a refusal), and the code says
/// `refusal` in the command's error and in the timeline JSON, in full and in short.
#[test]
fn a_shell_thicker_than_its_cans_radius_is_coded_a_refusal() {
    let mut s = Session::default();
    run(&mut s, "parameters.change", json!({"name":"wall", "expression":"2 mm"}));
    run(&mut s, "solid.cylinder", json!({"radius": 10, "height": 20, "body_name": "Can"}));
    let before = s.doc.as_ref().clone();
    let e = s.execute("solid.shell", &json!({"faces": [[0, 0, 20]], "thickness": 12})).unwrap_err();
    let code = e.error_code().cloned().unwrap_or_else(|| panic!("no code: {e}"));
    assert!((200..=204).contains(&code.kind), "{code}: {e}");
    assert_eq!(json!(code)["class"], "refusal", "{code}: {e}");

    let mut s = Session::new(before);
    let out = run(&mut s, "solid.shell", json!({"faces": [[0, 0, 20]], "thickness": "wall"}));
    let id = out["feature"].as_u64().unwrap();
    let inspected = run(&mut s, "timeline.inspect", json!({"feature":id}));
    assert!(inspected["error"].is_null() && inspected.get("error_code").is_none(), "{inspected}");
    s.execute("parameters.change", &json!({"name":"wall", "expression":"12 mm"})).ok();
    let inspected = run(&mut s, "timeline.inspect", json!({"feature":id}));
    assert_eq!(inspected["error_code"]["class"], "refusal", "{inspected}");
    assert_eq!(inspected["error_code"]["kind"], code.kind, "{inspected}");
    for detail in [json!(true), json!("summary")] {
        let doc = run(&mut s, "document.inspect", json!({"timeline":detail}));
        let row = doc["timeline"].as_array().unwrap().iter().find(|r| r["id"] == id).cloned().unwrap();
        assert_eq!(row["error_code"], inspected["error_code"], "{detail}: {row}");
    }
}
