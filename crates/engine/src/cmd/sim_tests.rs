//! `sim.*` (cmd/sim.rs) measured against values worked out by hand: a slot cut with a Ø6 flat end
//! mill, seeking back and forth, the refusals and `clear`.

use serde_json::{Value, json};

use crate::Session;
use crate::schema::ParamCheck;

/// A Ø6 flat end mill cuts 2 mm deep along X for 40 mm, in a 60 × 20 × 10 mm box: a stadium of
/// 40 × 6 plus a disc of radius 3, 2 mm deep.
const SLOT: &str = "T0 M6\nG21 G90\nG0 X10 Y10 Z5\nG1 Z-2 F100\nG1 X50 F300\nG0 Z5";
const SLOT_MOVES: u64 = 4;

fn slot_volume() -> f64 {
    (40.0 * 6.0 + std::f64::consts::PI * 3.0 * 3.0) * 2.0
}

fn setup_params(gcode: &str) -> Value {
    json!({
        "stock": {"box": {"min": [0, 0, -10], "max": [60, 20, 0]}},
        "tools": [{"id": 0, "diameter": 6, "flute_length": 20}],
        "gcode": gcode,
        "resolution": 0.1
    })
}

fn call(s: &mut Session, id: &str, params: &Value) -> Value {
    s.execute(id, params).unwrap_or_else(|e| panic!("{id} {params}: {e}"))
}

fn set_up(gcode: &str) -> Session {
    let mut s = Session::default();
    call(&mut s, "sim.setup", &setup_params(gcode));
    s
}

#[test]
fn a_slot_removes_its_analytic_volume() {
    let mut s = set_up(SLOT);
    let status = call(&mut s, "sim.status", &json!({}));
    assert_eq!(status["total"], SLOT_MOVES);
    assert_eq!(status["cursor"], 0);
    assert_eq!(status["removed_volume"], 0.0);
    let run = call(&mut s, "sim.run", &json!({}));
    assert_eq!(run["at"], SLOT_MOVES);
    assert_eq!(run["done"], true);
    let removed = run["removed_volume"].as_f64().unwrap_or(f64::NAN);
    let expected = slot_volume();
    assert!((removed - expected).abs() <= 0.01 * expected, "removed {removed} mm^3, analytic {expected} mm^3");
    let status = call(&mut s, "sim.status", &json!({}));
    let stock = status["stock_volume"].as_f64().unwrap_or(f64::NAN);
    assert!((stock - 12000.0).abs() < 1e-6, "box volume {stock}");
    assert_eq!(status["removed_volume"].as_f64(), Some(removed));
}

#[test]
fn running_back_matches_a_fresh_run() {
    let gcode = format!("{SLOT}\nG0 Y15\nG1 Z-4 F100\nG1 X10 F300\nG0 Z5");
    let mut s = set_up(&gcode);
    call(&mut s, "sim.run", &json!({"to": 7}));
    let back = call(&mut s, "sim.run", &json!({"to": 3}));
    assert_eq!(back["at"], 3);
    assert_eq!(back["done"], false);

    let mut fresh = set_up(&gcode);
    let direct = call(&mut fresh, "sim.run", &json!({"to": 3}));
    assert_eq!(back["removed_volume"], direct["removed_volume"]);
    let hash = |s: &Session| s.sim_stock().map(|st| st.state_hash());
    assert!(hash(&s).is_some());
    assert_eq!(hash(&s), hash(&fresh));
}

#[test]
fn the_default_resolution_is_the_finest_that_fits_half_the_default_cap() {
    let mut s = Session::default();
    let mut params = setup_params(SLOT);
    if let Some(o) = params.as_object_mut() {
        o.remove("resolution");
    }
    let setup = call(&mut s, "sim.setup", &params);
    assert_eq!(setup["h"].as_f64(), Some(0.05));
    assert_eq!(setup["memory_estimate"]["budget"].as_u64(), Some(512 << 20));
    let estimate = setup["memory_estimate"]["bytes"].as_u64().unwrap_or(u64::MAX);
    assert!(estimate <= setup["memory_estimate"]["budget"].as_u64().unwrap_or(0));
}

#[test]
fn setup_lists_the_gcode_problems_and_still_runs() {
    let gcode = format!("{SLOT}\nG123 X5");
    let mut s = Session::default();
    let setup = call(&mut s, "sim.setup", &setup_params(&gcode));
    assert_eq!(setup["diag_count"], 1);
    assert_eq!(setup["diagnostics"][0]["severity"], "error");
    let run = call(&mut s, "sim.run", &json!({}));
    assert_eq!(run["done"], true);
}

#[test]
fn run_and_status_need_a_setup() {
    let mut s = Session::default();
    assert!(s.execute("sim.run", &json!({})).is_err());
    assert!(s.execute("sim.status", &json!({})).is_err());
    assert!(s.sim_stock().is_none());
}

#[test]
fn the_sim_commands_do_not_read_the_model() {
    for id in ["sim.setup", "sim.run", "sim.status", "sim.clear"] {
        let spec = crate::cmd::find_command(id).unwrap_or_else(|| panic!("{id} is not registered"));
        assert!(!spec.reads_model, "{id} reads only its own state, so it must not force a model refresh");
    }
}

#[test]
fn hostile_sim_params_are_refused_not_panicked() {
    let good = setup_params(SLOT);
    let with = |key: &str, value: Value| {
        let mut p = good.clone();
        p[key] = value;
        p
    };
    let mut no_tools = good.clone();
    if let Some(o) = no_tools.as_object_mut() {
        o.remove("tools");
    }
    let flat = |id: u32| json!({"id": id, "diameter": 6, "flute_length": 20});
    let cases: Vec<(&str, Value)> = vec![
        ("no tools key", no_tools),
        ("resolution zero", with("resolution", json!(0))),
        ("resolution negative", with("resolution", json!(-0.1))),
        ("resolution absurdly fine", with("resolution", json!(1e-9))),
        ("inverted box", with("stock", json!({"box": {"min": [0, 0, 0], "max": [-1, 20, 0]}}))),
        ("empty box", with("stock", json!({"box": {"min": [0, 0, 0], "max": [0, 20, 0]}}))),
        ("box beyond 100 m", with("stock", json!({"box": {"min": [0, 0, 0], "max": [1e9, 20, 0]}}))),
        ("box of two numbers", with("stock", json!({"box": {"min": [0, 0], "max": [1, 1]}}))),
        ("memory zero", with("memory_mb", json!(0))),
        ("memory over the cap", with("memory_mb", json!(2048))),
        (
            "memory too small for the grid",
            json!({
                "stock": {"box": {"min": [0, 0, -100], "max": [300, 200, 0]}},
                "tools": [flat(0)],
                "gcode": SLOT, "resolution": 0.05, "memory_mb": 1
            }),
        ),
        ("tool diameter zero", with("tools", json!([{"id": 0, "diameter": 0, "flute_length": 20}]))),
        ("tool negative flute", with("tools", json!([{"id": 0, "diameter": 6, "flute_length": -1}]))),
        ("unknown shape", with("tools", json!([{"id": 0, "diameter": 6, "flute_length": 20, "shape": "hexagon"}]))),
        ("bull without a corner radius", with("tools", json!([{"id": 0, "diameter": 6, "flute_length": 20, "shape": "bull"}]))),
        ("cone without an angle", with("tools", json!([{"id": 0, "diameter": 6, "flute_length": 20, "shape": "cone", "tip_diameter": 0}]))),
        ("corner radius on a ball", with("tools", json!([{"id": 0, "diameter": 6, "flute_length": 20, "shape": "ball", "corner_radius": 1}]))),
        ("tool listed twice", with("tools", json!([flat(0), flat(0)]))),
        ("gcode not text", with("gcode", json!(5))),
        ("rapid rate zero", with("rapid_mm_min", json!(0))),
        ("rapid rate negative", with("rapid_mm_min", json!(-5000))),
        ("rapid rate text", with("rapid_mm_min", json!("fast"))),
    ];
    for (name, params) in cases {
        let mut s = Session::default();
        assert!(s.execute("sim.setup", &params).is_err(), "{name} was accepted");
        assert!(s.sim_stock().is_none(), "{name} left a simulation behind");
    }
    let mut strict = Session { param_check: ParamCheck::Strict, ..Session::default() };
    assert!(strict.execute("sim.setup", &with("sketch", json!(1))).is_err(), "an unknown key was accepted in strict");
    let mut s = set_up(SLOT);
    for params in [
        json!({"to": 99}),
        json!({"to": -1}),
        json!({"to": "begin"}),
        json!({"budget_s": -1}),
        json!({"budget_s": "soon"}),
        json!({"time": -1}),
        json!({"time": "soon"}),
        json!({"time": 1, "to": 1}),
    ] {
        assert!(s.execute("sim.run", &params).is_err(), "{params} was accepted");
    }
    assert!(s.execute("sim.status", &json!({"limit": 5000000000u64, "offset": -1})).is_err());
    assert_eq!(call(&mut s, "sim.status", &json!({"limit": 5000000000u64})).get("cursor"), Some(&json!(0)));
    // A line a billion mm long is cut in a bounded number of pieces, and cuts nothing above the box.
    let mut far = set_up("T0 M6\nG0 X0 Y10 Z5\nG1 X1000000000 F600");
    assert_eq!(call(&mut far, "sim.run", &json!({}))["done"], true);
    assert_eq!(call(&mut far, "sim.status", &json!({}))["removed_volume"], 0.0);
}

#[test]
fn clear_drops_the_simulation() {
    let mut s = set_up(SLOT);
    call(&mut s, "sim.run", &json!({}));
    assert_eq!(call(&mut s, "sim.clear", &json!({}))["cleared"], true);
    assert!(s.sim_stock().is_none());
    assert!(s.execute("sim.status", &json!({})).is_err());
    assert_eq!(call(&mut s, "sim.clear", &json!({}))["cleared"], false);
}

/// An arc at Z -9 around the origin, from (10, 0): a full circle of six turns. In a 40 × 40 × 10 box.
fn arc_setup() -> Value {
    json!({
        "stock": {"box": {"min": [-20, -20, -10], "max": [20, 20, 0]}},
        "tools": [{"id": 0, "diameter": 6, "flute_length": 20}],
        "gcode": "T0 M6\nG21 G90\nG0 X10 Y0 Z5\nG1 Z-9 F100\nG3 X10 Y0 I-10 J0 P6 F200\nG0 Z5",
        "resolution": 0.2
    })
}

#[test]
fn a_budgeted_arc_is_cut_one_step_per_call() {
    let mut s = Session::default();
    call(&mut s, "sim.setup", &arc_setup());
    // With no budget left, each call still cuts one step: the arc is entered and left part-cut.
    // The plunge before the arc is in pieces too, so wait for the call that is inside the arc.
    let mut reply = call(&mut s, "sim.run", &json!({"budget_s": 0.0}));
    let mut calls = 1;
    while (reply["partial"].is_null() || reply["at"] != 2) && calls < 50 {
        reply = call(&mut s, "sim.run", &json!({"budget_s": 0.0}));
        calls += 1;
    }
    assert!(!reply["partial"].is_null(), "no call stopped inside the arc: {reply}");
    assert_eq!(reply["at"], 2, "the arc counts as cut only at its last revolution: {reply}");
    assert_eq!(reply["partial"]["step"], 1, "{reply}");
    let steps = reply["partial"]["steps"].as_u64().unwrap_or(0);
    assert!(steps >= 2, "{reply}");
    let mut done = reply;
    let mut more = 0;
    while done["done"] != json!(true) {
        done = call(&mut s, "sim.run", &json!({"budget_s": 0.0}));
        more += 1;
        assert!(more <= 2 * steps + 4, "the run does not finish: {done}");
    }
    let mut whole = Session::default();
    call(&mut whole, "sim.setup", &arc_setup());
    call(&mut whole, "sim.run", &json!({}));
    assert_eq!(s.sim_stock().map(|st| st.state_hash()), whole.sim_stock().map(|st| st.state_hash()));
}

#[test]
fn a_budget_error_stops_the_run_and_status_says_why() {
    // At 0.2 mm the empty box is about 1.0 MB of a 1 MB cap, so the first cut that splits rays
    // overflows it. The run stops there with the error kept: a forward run then refuses again and
    // does not retry the same move.
    let mut s = Session::default();
    let params = json!({
        "stock": {"box": {"min": [0, 0, -10], "max": [60, 20, 0]}},
        "tools": [{"id": 0, "diameter": 6, "flute_length": 20}],
        "gcode": "T0 M6\nG0 X10 Y10 Z5\nG1 Z-8 F100\nG1 X50 F300\nG0 Z5",
        "resolution": 0.2, "memory_mb": 1
    });
    call(&mut s, "sim.setup", &params);
    let err = s.execute("sim.run", &json!({})).expect_err("the cut must stop on the budget");
    assert!(err.to_string().contains("byte budget"), "{err}");
    let status = call(&mut s, "sim.status", &json!({}));
    assert!(status["error"].as_str().unwrap_or("").contains("byte budget"), "the stop is not in the status: {status}");
    let cursor = status["cursor"].clone();
    // The failed move is taken back whole, so the removed volume is the one the completed moves give.
    let mut fresh = Session::default();
    call(&mut fresh, "sim.setup", &params);
    let at_cursor = call(&mut fresh, "sim.run", &json!({"to": cursor}));
    assert_eq!(status["removed_volume"], at_cursor["removed_volume"], "{status}");
    assert!(s.execute("sim.run", &json!({})).is_err(), "a stopped run must not retry on its own");
    let after = call(&mut s, "sim.status", &json!({}));
    assert_eq!(after["cursor"], cursor);
    // A backward seek clears the stop and the run goes on from the start.
    let back = call(&mut s, "sim.run", &json!({"to": 0}));
    assert_eq!(back["at"], 0);
    assert!(call(&mut s, "sim.status", &json!({}))["error"].is_null());
}

#[test]
fn a_rapid_before_the_first_tool_change_is_not_an_error() {
    let gcode = "G0 X0 Y0 Z5\nT1 M6\nG0 X10 Y10 Z5\nG1 Z-2 F100\nG1 X30 F300\nG0 Z5";
    let params = json!({
        "stock": {"box": {"min": [0, 0, -10], "max": [60, 20, 0]}},
        "tools": [{"id": 1, "diameter": 6, "flute_length": 20}],
        "gcode": gcode, "resolution": 0.1
    });
    let mut s = Session::default();
    call(&mut s, "sim.setup", &params);
    let run = call(&mut s, "sim.run", &json!({}));
    assert_eq!(run["done"], true);
    assert!(run["removed_volume"].as_f64().unwrap_or(0.0) > 0.0, "{run}");
}

#[test]
fn a_library_listing_extra_flat_and_ball_tools_is_accepted() {
    let params = json!({
        "stock": {"box": {"min": [0, 0, -10], "max": [60, 20, 0]}},
        "tools": [
            {"id": 1, "diameter": 6, "flute_length": 20},
            {"id": 2, "diameter": 3, "flute_length": 10},
            {"id": 3, "diameter": 5, "flute_length": 15, "shape": "ball"}
        ],
        "gcode": "T1 M6\nG0 X10 Y10 Z5\nG1 Z-2 F100\nG1 X30 F300\nG0 Z5",
        "resolution": 0.1
    });
    let mut s = Session::default();
    call(&mut s, "sim.setup", &params);
    let run = call(&mut s, "sim.run", &json!({}));
    assert_eq!(run["done"], true);
    assert!(run["removed_volume"].as_f64().unwrap_or(0.0) > 0.0, "{run}");
}

#[test]
fn a_program_in_rs274_shape_runs() {
    // The header, M5, the tool change, the length offset and an arc, as the usual post writes them.
    let gcode = "%\nO0001 (SLOT TEST)\nN10 G21 G90 G17\nN20 M5\nN30 T1 M6\nN40 G43 H1\nN50 G0 X10 Y10 Z5\nN60 G1 Z-2 F100\nN70 G2 X20 Y10 I5 J0 F200\nN80 G0 Z5\nN90 M5\nM30\n%";
    let params = json!({
        "stock": {"box": {"min": [0, 0, -10], "max": [60, 20, 0]}},
        "tools": [{"id": 1, "diameter": 6, "flute_length": 20}],
        "gcode": gcode, "resolution": 0.1
    });
    let mut s = Session::default();
    call(&mut s, "sim.setup", &params);
    let run = call(&mut s, "sim.run", &json!({}));
    assert_eq!(run["done"], true);
    assert!(run["removed_volume"].as_f64().unwrap_or(0.0) > 0.0, "{run}");
}

#[test]
fn initial_tool_cuts_a_program_without_a_tool_change() {
    let params = json!({
        "stock": {"box": {"min": [0, 0, -10], "max": [60, 20, 0]}},
        "tools": [{"id": 1, "diameter": 6, "flute_length": 20}],
        "gcode": "G0 X10 Y10 Z5\nG1 Z-2 F100\nG1 X30 F300\nG0 Z5",
        "initial_tool": 1, "resolution": 0.1
    });
    let mut s = Session::default();
    call(&mut s, "sim.setup", &params);
    let run = call(&mut s, "sim.run", &json!({}));
    assert!(run["removed_volume"].as_f64().unwrap_or(0.0) > 0.0, "{run}");
}

#[test]
fn a_move_with_an_undefined_tool_is_a_diagnostic_not_a_setup_error() {
    let params = json!({
        "stock": {"box": {"min": [0, 0, -10], "max": [60, 20, 0]}},
        "tools": [{"id": 1, "diameter": 6, "flute_length": 20}],
        "gcode": "T1 M6\nG0 X10 Y10 Z5\nG1 Z-2 F100\nT2 M6\nG1 X30 F300",
        "resolution": 0.1
    });
    let mut s = Session::default();
    let setup = call(&mut s, "sim.setup", &params);
    assert_eq!(setup["diag_count"], 0, "{setup}");
    let run = call(&mut s, "sim.run", &json!({}));
    assert_eq!(run["done"], true);
    let status = call(&mut s, "sim.status", &json!({}));
    assert_eq!(status["diag_count"], 1, "{status}");
    assert!(status["diagnostics"][0]["msg"].as_str().unwrap_or("").contains("tool 2"), "{status}");
}

#[test]
fn reached_says_the_target_was_cut_and_done_says_the_program_was() {
    let mut s = set_up(SLOT);
    let part = call(&mut s, "sim.run", &json!({"to": 2}));
    assert_eq!((part["reached"].clone(), part["done"].clone()), (json!(true), json!(false)), "{part}");
    let end = call(&mut s, "sim.run", &json!({}));
    assert_eq!((end["reached"].clone(), end["done"].clone()), (json!(true), json!(true)), "{end}");
}

#[test]
fn a_zero_budget_still_cuts_a_move() {
    let mut s = set_up(SLOT);
    let run = call(&mut s, "sim.run", &json!({"budget_s": 0.0}));
    assert!(run["at"].as_u64().unwrap_or(0) >= 1, "a zero budget must still make progress: {run}");
}

#[test]
fn a_backward_seek_redraws_the_whole_surface() {
    // Two slots, the second 160 mm along X. Seeking back to move 4 restores the material of the second
    // slot, so its chunks must be dirty for the UI to redraw, not only the chunks the cut touched.
    let gcode = "T0 M6\nG21 G90\nG0 X10 Y10 Z5\nG1 Z-2 F100\nG1 X20 F300\nG0 Z5\nG0 X170 Y10 Z5\nG1 Z-2 F100\nG1 X180 F300\nG0 Z5";
    let params = json!({
        "stock": {"box": {"min": [0, 0, -10], "max": [200, 20, 0]}},
        "tools": [{"id": 0, "diameter": 6, "flute_length": 20}],
        "gcode": gcode, "resolution": 0.1
    });
    let mut s = Session::default();
    call(&mut s, "sim.setup", &params);
    let end = call(&mut s, "sim.run", &json!({}));
    assert_eq!(end["done"], true);
    let touched = s.sim_stock().map(|st| st.touched_chunks()).unwrap_or_default();
    // The UI has drawn the cut.
    s.sim_take_dirty();
    let back = call(&mut s, "sim.run", &json!({"to": 4}));
    assert_ne!(back["revision"], end["revision"], "a rebuild changes the revision: {back}");
    assert_eq!(Some(back["revision"].as_u64().unwrap_or(0)), Some(s.sim_revision()));
    let dirty = s.sim_take_dirty();
    // The dirty chunks are the chunks the cut touched, and only those: a 200 x 20 x 10 mm stock at 0.1 mm
    // has 1764 chunks; each sweep reaches its chunks plus a two-node margin, so two slots touch about 108.
    assert_eq!(dirty, touched, "a backward seek redraws the union of touched chunks");
    assert!(!touched.is_empty() && touched.len() < 200, "{} touched", touched.len());
    // Cells of 0.1 mm, 32 to a chunk: x = 175 is about chunk 54.
    assert!(dirty.iter().any(|c| (53..=55).contains(&c[0])), "the second slot's chunks must redraw: {dirty:?}");
    assert!(s.sim_take_dirty().is_empty(), "taking the dirty chunks forgets them");
}

#[test]
fn a_flat_setup_has_no_chunks_the_ui_must_mesh_until_a_cut_reaches_them() {
    // Redraws follow the cut: a box with no cut dirties nothing, and a backward seek to the start
    // dirties exactly the chunks the cut had touched.
    let mut s = set_up(SLOT);
    call(&mut s, "sim.run", &json!({}));
    let touched = s.sim_stock().map(|st| st.touched_chunks()).unwrap_or_default();
    s.sim_take_dirty();
    call(&mut s, "sim.run", &json!({"to": 0}));
    assert_eq!(s.sim_take_dirty(), touched);
}

#[test]
fn a_fresh_setup_has_nothing_to_redraw() {
    // The UI draws the uncut box itself: no chunk is dirty until a cut reaches it.
    let mut s = set_up(SLOT);
    assert!(s.sim_take_dirty().is_empty(), "a fresh box has dirty chunks");
    call(&mut s, "sim.run", &json!({}));
    assert!(!s.sim_take_dirty().is_empty(), "the cut must dirty the chunks it reached");
}

#[test]
fn cutting_moves_before_the_first_tool_change_are_an_error_with_a_count() {
    let params = json!({
        "stock": {"box": {"min": [0, 0, -10], "max": [60, 20, 0]}},
        "tools": [{"id": 1, "diameter": 6, "flute_length": 20}],
        "gcode": "G0 X10 Y10 Z5\nG1 Z-3 F100\nG1 X40\nG0 Z5\nT1 M6\nG0 X10 Y10 Z5\nG1 Z-2 F100\nG1 X30 F300\nG0 Z5",
        "resolution": 0.1
    });
    let mut s = Session::default();
    let setup = call(&mut s, "sim.setup", &params);
    let errors: Vec<String> = setup["diagnostics"]
        .as_array()
        .map(|d| d.iter().filter(|x| x["severity"] == "error").filter_map(|x| x["msg"].as_str().map(String::from)).collect())
        .unwrap_or_default();
    assert!(errors.iter().any(|m| m.contains("2 cutting moves before the first M6")), "{setup}");
}

#[test]
fn a_flat_tool_accepts_a_zero_corner_radius() {
    let mut params = setup_params(SLOT);
    params["tools"] = json!([{"id": 0, "diameter": 6, "flute_length": 20, "corner_radius": 0}]);
    let mut s = Session::default();
    call(&mut s, "sim.setup", &params);
    assert_eq!(call(&mut s, "sim.run", &json!({}))["done"], true);
}

#[test]
fn tool_and_box_entries_refuse_unknown_keys() {
    let mut tool = setup_params(SLOT);
    tool["tools"] = json!([{"id": 0, "diameter": 6, "flute_length": 20, "colour": "red"}]);
    assert!(Session::default().execute("sim.setup", &tool).is_err(), "an unknown tool key was accepted");
    let mut bx = setup_params(SLOT);
    bx["stock"] = json!({"box": {"min": [0, 0, -10], "max": [60, 20, 0], "margin": 1}});
    assert!(Session::default().execute("sim.setup", &bx).is_err(), "an unknown box key was accepted");
}

#[test]
fn revisions_never_repeat_across_sessions() {
    let mut a = set_up(SLOT);
    let mut b = set_up(SLOT);
    let ra = call(&mut a, "sim.run", &json!({}))["revision"].as_u64().unwrap_or(0);
    let rb = call(&mut b, "sim.run", &json!({}))["revision"].as_u64().unwrap_or(0);
    assert_ne!(ra, rb, "two simulations share revision {ra}");
    let again = call(&mut a, "sim.setup", &setup_params(SLOT));
    assert!(again.is_object());
    let ra2 = call(&mut a, "sim.run", &json!({}))["revision"].as_u64().unwrap_or(0);
    assert!(ra2 > ra && ra2 != rb, "a new setup must hand out a new revision: {ra} {ra2} {rb}");
}

#[test]
fn moves_take_their_length_over_their_feed() {
    // 60 mm at F600 is 6 s. The rapid back is 60 mm at the default 5000 mm/min. The first move has no
    // start point, so it takes no time.
    let gcode = "T0 M6\nG21 G90\nG0 X0 Y10 Z5\nG1 X60 F600\nG0 X0 Y10 Z5";
    let mut s = set_up(gcode);
    let status = call(&mut s, "sim.status", &json!({}));
    let total = status["total_time"].as_f64().unwrap_or(f64::NAN);
    assert!((total - (6.0 + 60.0 * 60.0 / 5000.0)).abs() < 1e-9, "total {total}");
    assert_eq!(status["time"], 0.0);
    let run = call(&mut s, "sim.run", &json!({"time": 6.0}));
    assert_eq!((run["at"].clone(), run["reached"].clone()), (json!(2), json!(true)), "{run}");
    assert_eq!(run["time"], 6.0);
    assert_eq!(call(&mut s, "sim.run", &json!({"time": 0}))["removed_volume"], 0.0);
    // A rapid rate of 600 mm/min makes the same rapid 6 s.
    let mut params = setup_params(gcode);
    params["rapid_mm_min"] = json!(600);
    let mut slow = Session::default();
    call(&mut slow, "sim.setup", &params);
    let total = call(&mut slow, "sim.status", &json!({}))["total_time"].as_f64().unwrap_or(f64::NAN);
    assert!((total - 12.0).abs() < 1e-9, "total {total}");
}

#[test]
fn an_arc_takes_its_own_length() {
    // A half circle of radius 10 at F600 is 31.4 mm, so pi seconds at 10 mm/s.
    let mut s = set_up("T0 M6\nG21 G90\nG0 X10 Y0 Z5\nG3 X-10 Y0 I-10 J0 F600");
    let total = call(&mut s, "sim.status", &json!({}))["total_time"].as_f64().unwrap_or(f64::NAN);
    assert!((total - std::f64::consts::PI).abs() < 1e-9, "total {total}");
}

#[test]
fn run_by_time_stops_part_way_along_a_long_cut() {
    // A plunge of 7 mm at F100 (4.2 s), then 50 mm at F300 (10 s). At 9.2 s the cut is half way along.
    let gcode = "T0 M6\nG21 G90\nG0 X0 Y10 Z5\nG1 Z-2 F100\nG1 X50 F300\nG0 Z5";
    let removed_to = |to: u64| {
        let mut s = set_up(gcode);
        call(&mut s, "sim.run", &json!({"to": to}))["removed_volume"].as_f64().unwrap_or(f64::NAN)
    };
    let before = removed_to(2);
    let full = removed_to(3);
    let cut = full - before;
    assert!(cut > 0.0, "the long cut removes material: {before} to {full}");
    let mut s = set_up(gcode);
    let run = call(&mut s, "sim.run", &json!({"time": 9.2}));
    assert_eq!((run["at"].clone(), run["reached"].clone()), (json!(2), json!(true)), "{run}");
    assert!(!run["partial"].is_null(), "half way is inside the long move: {run}");
    let mid = run["removed_volume"].as_f64().unwrap_or(f64::NAN);
    assert!(((mid - before) - cut / 2.0).abs() <= 0.1 * cut, "half way removed {} of {cut} mm^3", mid - before);
    assert_eq!(s.sim_time().map(|(t, _)| t), Some(9.2));
}

#[test]
fn the_tool_sits_on_its_move_at_a_mid_time() {
    let gcode = "T0 M6\nG21 G90\nG0 X0 Y10 Z5\nG1 Z-2 F100\nG1 X50 F300\nG0 Z5";
    let mut s = set_up(gcode);
    call(&mut s, "sim.run", &json!({"time": 9.2}));
    let (id, tip, axis) = s.sim_tool_pose().unwrap_or_else(|| panic!("no tool pose at 9.2 s"));
    assert_eq!(id, 0);
    // Half way from (0, 10, -2) to (50, 10, -2).
    for (got, want) in tip.iter().zip([25.0, 10.0, -2.0]) {
        assert!((got - want).abs() < 1e-6, "tip {tip:?}");
    }
    assert_eq!(axis, [0.0, 0.0, 1.0]);
    let status = call(&mut s, "sim.status", &json!({}));
    assert_eq!(status["tool"]["id"], 0);
    assert!((status["tool"]["tip"][0].as_f64().unwrap_or(f64::NAN) - 25.0).abs() < 1e-6, "{status}");
}

#[test]
fn a_move_with_no_tool_has_no_tool_pose() {
    let mut s = set_up("G0 X10 Y10 Z5");
    let status = call(&mut s, "sim.status", &json!({}));
    assert!(status["tool"].is_null(), "{status}");
    assert!(s.sim_tool_pose().is_none());
}

/// A program of many short moves, so the checkpoints every 256 moves come into play.
fn long_program(moves: usize) -> String {
    let mut g = String::from("T0 M6\nG21 G90\nG0 X10 Y10 Z5\nG1 Z-2 F600\n");
    for k in 0..moves {
        let x = 10.0 + ((k * 37) % 7) as f64;
        let y = 8.0 + ((k * 5) % 3) as f64 * 2.0;
        g.push_str(&format!("G1 X{x} Y{y} F600\n"));
    }
    g
}

fn long_setup(gcode: &str, memory_mb: u64) -> Session {
    let mut s = Session::default();
    let params = json!({
        "stock": {"box": {"min": [0, 0, -10], "max": [40, 20, 0]}},
        "tools": [{"id": 0, "diameter": 6, "flute_length": 20}],
        "gcode": gcode, "resolution": 0.25, "memory_mb": memory_mb
    });
    call(&mut s, "sim.setup", &params);
    s
}

#[test]
fn backward_seeks_by_time_match_a_fresh_run_across_checkpoints() {
    // A 4 MB cap keeps 1 MB of checkpoints, so as the program goes on they are thinned and spaced out.
    // Each backward seek must give the stock a fresh run to the same time gives.
    let gcode = long_program(2100);
    let mut s = long_setup(&gcode, 4);
    let total = call(&mut s, "sim.status", &json!({}))["total_time"].as_f64().unwrap_or(f64::NAN);
    call(&mut s, "sim.run", &json!({}));
    for fraction in [0.8, 0.2, 0.6, 0.35] {
        let t = total * fraction;
        let restores = s.sim_checkpoint_restores().unwrap_or(0);
        let revision = s.sim_revision();
        let back = call(&mut s, "sim.run", &json!({"time": t}));
        if s.sim_checkpoint_restores().unwrap_or(0) > restores {
            assert_eq!(s.sim_revision(), revision, "a checkpoint restore keeps the revision, the chunks it changed are dirty");
        }
        let mut fresh = long_setup(&gcode, 4);
        let direct = call(&mut fresh, "sim.run", &json!({"time": t}));
        assert_eq!(back["at"], direct["at"], "moves cut at {fraction}: {back}");
        assert_eq!(back["removed_volume"], direct["removed_volume"], "removed at {fraction}: {back}");
        let hash = |s: &Session| s.sim_stock().map(|st| st.state_hash());
        assert_eq!(hash(&s), hash(&fresh), "stock at {fraction}");
    }
    assert!(s.sim_checkpoint_restores().unwrap_or(0) > 0, "no backward seek used a checkpoint");
}

#[test]
fn a_rewind_inside_a_move_is_taken_back_in_place() {
    // The run stops 9.2 s into the long cut, then goes back to 7.0 s, inside the same move. It must give
    // the stock a fresh run to 7.0 s gives, under the same revision.
    let gcode = "T0 M6\nG21 G90\nG0 X0 Y10 Z5\nG1 Z-2 F100\nG1 X50 F300\nG0 Z5";
    let mut s = set_up(gcode);
    let first = call(&mut s, "sim.run", &json!({"time": 9.2}));
    let back = call(&mut s, "sim.run", &json!({"time": 7.0}));
    assert_eq!(back["revision"], first["revision"], "a rewind inside a move keeps the revision: {back}");
    let mut fresh = set_up(gcode);
    let direct = call(&mut fresh, "sim.run", &json!({"time": 7.0}));
    assert_eq!(back["at"], direct["at"], "{back}");
    assert_eq!(back["partial"], direct["partial"], "{back}");
    assert_eq!(back["removed_volume"], direct["removed_volume"], "{back}");
    let hash = |s: &Session| s.sim_stock().map(|st| st.state_hash());
    assert_eq!(hash(&s), hash(&fresh));
}

#[test]
fn a_chunk_a_rewind_inside_a_move_drops_stays_dirty_across_a_restore() {
    // The last move runs into chunks no earlier move cut. Seeks go back inside that move, which takes
    // back the chunks it reached: they are no longer cut, so only their dirtiness says their meshes must
    // go. No dirty chunk is taken in between, so the restore that follows must keep them dirty.
    let gcode = format!("{}G1 X38 Y2 F300\n", long_program(600));
    let mut s = long_setup(&gcode, 4);
    let total = call(&mut s, "sim.status", &json!({}))["total_time"].as_f64().unwrap_or(f64::NAN);
    call(&mut s, "sim.run", &json!({"time": total - 0.5}));
    // The last move takes about 5.6 s in 15 steps. A rewind of 0.5 s takes back one step, and the steps
    // before it reach the same chunks; 4 s takes it back to about step 4, which drops the chunks the later
    // steps were the first to reach.
    let back = call(&mut s, "sim.run", &json!({"time": total - 4.0}));
    assert_eq!(back["at"], 602, "the rewind stays inside the last move: {back}");
    let (dirty, touched) =
        (s.sim_stock().map(|st| st.dirty_chunks()).unwrap_or_default(), s.sim_stock().map(|st| st.touched_chunks()).unwrap_or_default());
    let dropped: Vec<_> = dirty.into_iter().filter(|c| !touched.contains(c)).collect();
    assert!(!dropped.is_empty(), "the rewind must leave a dirty chunk that is no longer cut");
    let restores = s.sim_checkpoint_restores().unwrap_or(0);
    call(&mut s, "sim.run", &json!({"time": total * 0.5}));
    assert!(s.sim_checkpoint_restores().unwrap_or(0) > restores, "the last seek must restore a checkpoint");
    let dirty = s.sim_stock().map(|st| st.dirty_chunks()).unwrap_or_default();
    for c in &dropped {
        assert!(dirty.contains(c), "chunk {c:?}, dropped by the rewind, is not dirty after the restore");
    }
}

#[test]
fn a_checkpoint_over_the_cap_is_refused_and_the_spacing_grows() {
    // The first checkpoint is the whole stock, so a stock over a quarter of the budget keeps none: the
    // run goes on to the end without a panic, and the spacing between checkpoints doubles at each refusal.
    let gcode = long_program(600);
    let mut found = None;
    for h in [0.25, 0.3, 0.35, 0.4, 0.5, 0.6, 0.7, 0.8, 1.0] {
        let mut s = Session::default();
        let params = json!({
            "stock": {"box": {"min": [0, 0, -10], "max": [40, 20, 0]}},
            "tools": [{"id": 0, "diameter": 6, "flute_length": 20}],
            "gcode": gcode, "resolution": h, "memory_mb": 1
        });
        let setup = call(&mut s, "sim.setup", &params);
        let (bytes, budget) = (setup["memory_estimate"]["bytes"].as_u64().unwrap_or(0), setup["memory_estimate"]["budget"].as_u64().unwrap_or(0));
        if bytes * 4 > budget && bytes <= budget {
            found = Some(s);
            break;
        }
    }
    let Some(mut s) = found else { panic!("no resolution puts the box over a quarter of a 1 MB budget") };
    let run = call(&mut s, "sim.run", &json!({}));
    assert_eq!(run["done"], true, "{run}");
    assert!(s.sim_checkpoint_every().unwrap_or(0) > 256, "the refused checkpoints must space them further apart");
}

/// Wall time of a whole run of a long program of short lines, on a stock at the default resolution.
/// Not a check: `cargo test -p solidcraft-engine --lib -- --ignored --nocapture sim_full_run_timing`.
#[test]
#[ignore]
fn sim_full_run_timing() {
    let mut g = String::from("T0 M6\nG21 G90\nG0 X5 Y5 Z5\nG1 Z-2 F600\n");
    for k in 0..20000usize {
        let (row, col) = (k / 200, k % 200);
        let x = if row % 2 == 0 { col } else { 199 - col };
        g.push_str(&format!("G1 X{:.2} Y{:.2} F1200\n", 5.0 + x as f64 * 0.45, 5.0 + row as f64 * 0.45));
    }
    let params = json!({
        "stock": {"box": {"min": [0, 0, -30], "max": [100, 100, 0]}},
        "tools": [{"id": 0, "diameter": 6, "flute_length": 20}],
        "gcode": g
    });
    let mut s = Session::default();
    let setup_started = std::time::Instant::now();
    let setup = call(&mut s, "sim.setup", &params);
    let setup_time = setup_started.elapsed();
    let run_started = std::time::Instant::now();
    let run = call(&mut s, "sim.run", &json!({}));
    let run_time = run_started.elapsed();
    assert_eq!(run["done"], true);
    eprintln!(
        "sim timing: {} moves at h {}, setup {:?}, full run {:?}, removed {} mm^3",
        setup["moves"], setup["h"], setup_time, run_time, run["removed_volume"]
    );
}
