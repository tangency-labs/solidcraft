#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::{Value, json};
use std::f64::consts::PI;

use super::super::route_plan::{self as rp, Kind, Spec};
use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn f(v: &Value) -> f64 {
    v.as_f64().unwrap_or_else(|| panic!("not a number: {v}"))
}

fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs().max(1e-12)
}

/// Two made-up routes that exercise the planner: `a` is a header with two tees, two branch
/// reducers, two valves and a capped end; `b` is a run with two valves and a weld.
fn line(name: &str) -> Spec {
    let text = match name {
        "a" => include_str!("route_fixtures/line-a.json"),
        _ => include_str!("route_fixtures/line-b.json"),
    };
    serde_json::from_str(text).unwrap()
}

fn lengths(pieces: &[rp::Piece]) -> Vec<(String, f64)> {
    pieces.iter().filter(|p| p.kind == Kind::Pipe).map(|p| (p.name.clone(), p.length.unwrap())).collect()
}

fn count(pieces: &[rp::Piece], k: Kind) -> usize {
    pieces.iter().filter(|p| p.kind == k).count()
}

// ---- the planner ----------------------------------------------------------------------------

#[test]
fn the_header_plans_its_24_pieces_and_12_cut_lengths() {
    let pieces = rp::plan(&line("a")).unwrap();
    assert_eq!(pieces.len(), 24);
    assert_eq!(
        (
            count(&pieces, Kind::Elbow),
            count(&pieces, Kind::Tee),
            count(&pieces, Kind::Reducer),
            count(&pieces, Kind::Valve),
            count(&pieces, Kind::Cap)
        ),
        (5, 2, 2, 2, 1)
    );
    // Cut lengths in mm: 600 mm runs less each tee's and elbow's centre-to-end, 200 mm branches
    // less their tee's outlet, and the two reducer spans' following pipes.
    let mut want = vec![561.9, 523.8, 223.8, 223.8, 223.8, 223.8, 223.8, 261.9, 161.9, 200.0, 161.9, 200.0];
    let mut got: Vec<f64> = lengths(&pieces).into_iter().map(|x| x.1).collect();
    want.sort_by(f64::total_cmp);
    got.sort_by(f64::total_cmp);
    assert_eq!(got.len(), want.len());
    for (g, w) in got.iter().zip(want) {
        assert!((g - w).abs() < 1e-4, "{g} vs {w}");
    }
}

#[test]
fn the_run_plans_its_14_pieces_and_8_cut_lengths() {
    let pieces = rp::plan(&line("b")).unwrap();
    assert_eq!(pieces.len(), 14);
    let mut want = vec![1000.0, 400.0, 361.9, 423.8, 423.8, 423.8, 461.9, 500.0];
    let mut got: Vec<f64> = lengths(&pieces).into_iter().map(|x| x.1).collect();
    want.sort_by(f64::total_cmp);
    got.sort_by(f64::total_cmp);
    assert_eq!(got.len(), want.len());
    for (g, w) in got.iter().zip(want) {
        assert!((g - w).abs() < 1e-4, "{g} vs {w}");
    }
    assert_eq!((count(&pieces, Kind::Elbow), count(&pieces, Kind::Valve)), (4, 2));
}

#[test]
fn moving_a_corner_carries_the_leg_beyond_it() {
    let mut spec = line("a");
    spec.start = Some([0.0, 0.0, 0.0]);
    let before = rp::plan(&spec).unwrap();
    let corner = [1500.0, 300.0, 0.0];
    let (after, moved) = rp::move_point(&spec, corner, [0.0, 0.0, 100.0], true).unwrap();
    assert_eq!(moved, 5, "the corner and the four points beyond it toward the capped end");
    let pieces = rp::plan(&after).unwrap();
    assert_eq!(pieces.len(), 24, "the fittings and pipes are the same set");
    // The capped end went with the leg.
    let at = |ps: &[rp::Piece]| ps.iter().find(|p| p.kind == Kind::Cap).and_then(|p| p.matrix).unwrap();
    let (a, b) = (at(&before), at(&pieces));
    assert!((b[11] - a[11] - 100.0).abs() < 1e-9, "{a:?} {b:?}");
    // A move without carry leaves the points beyond it where they are.
    let (_, one) = rp::move_point(&spec, corner, [0.0, 0.0, 10.0], false).unwrap();
    assert_eq!(one, 1);
}

#[test]
fn a_branch_takes_a_tee_and_a_size_change_needs_a_reducer() {
    let tee = json!({"name": "T", "nps": 1.0, "segments": [{"a": [0, 0, 0], "b": [300, 0, 0]}, {"a": [300, 0, 0], "b": [600, 0, 0]},
                     {"a": [300, 0, 0], "b": [300, 200, 0], "nps": 0.75}]});
    let pieces = rp::plan(&serde_json::from_value(tee).unwrap()).unwrap();
    let t = pieces.iter().find(|p| p.kind == Kind::Tee).unwrap();
    assert_eq!(t.lib.as_deref(), Some("tee_1x0_75"));
    let l: Vec<f64> = lengths(&pieces).into_iter().map(|x| x.1).collect();
    assert_eq!(l, vec![300.0 - 38.1, 300.0 - 38.1, 200.0 - 38.1]);
    let step = json!({"name": "S", "segments": [{"a": [0, 0, 0], "b": [300, 0, 0], "nps": 2.0}, {"a": [300, 0, 0], "b": [600, 0, 0], "nps": 1.0}]});
    let e = rp::plan(&serde_json::from_value(step).unwrap()).unwrap_err();
    assert!(e.contains("without a reducer"), "{e}");
}

#[test]
fn hostile_specs_are_refused_with_the_reason() {
    for (spec, why) in [
        (json!({"name": "", "nps": 1.0, "points": [[0, 0, 0], [1, 0, 0]]}), "name"),
        (json!({"name": "a b", "nps": 1.0, "points": [[0, 0, 0], [1, 0, 0]]}), "name"),
        (json!({"name": "R", "nps": 7.0, "points": [[0, 0, 0], [100, 0, 0]]}), "pipe table"),
        (json!({"name": "R", "nps": 1.0, "points": [[0, 0, 0]]}), "two points"),
        (json!({"name": "R", "points": [[0, 0, 0], [100, 0, 0]]}), "nps"),
        (json!({"name": "R", "nps": 1.0, "points": [[0, 0, 0], [1e12, 0, 0]]}), "finite"),
        (json!({"name": "R", "nps": 1.0, "points": [[0, 0, 0], [30, 0, 0], [30, 300, 0]]}), "overlap"),
        (json!({"name": "R", "nps": 0.5, "elbow": "short_radius", "points": [[0, 0, 0], [300, 0, 0], [300, 300, 0]]}), "elbow"),
        (json!({"name": "R", "nps": 1.0, "points": [[0, 0, 0], [300, 0, 0]], "fittings": [{"kind": "cap", "at": [150, 0, 0]}]}), "node"),
    ] {
        let e = serde_json::from_value::<Spec>(spec.clone()).map_err(|e| e.to_string()).and_then(|s| rp::plan(&s).map(|_| ()));
        let e = e.expect_err(&spec.to_string());
        assert!(e.contains(why), "{spec}: {e}");
    }
}

#[test]
fn a_weld_between_two_sizes_is_refused() {
    // The weld's node is excluded from the straight-run check, so the sizes must be compared
    // there too: a butt weld joins two pipes of one size.
    let spec: Spec = serde_json::from_value(json!({"name": "W", "segments": [
        {"a": [0, 0, 0], "b": [300, 0, 0], "nps": 2.0},
        {"a": [300, 0, 0], "b": [600, 0, 0], "nps": 1.0}
    ], "fittings": [{"kind": "weld", "at": [300, 0, 0]}]}))
    .unwrap();
    let e = rp::plan(&spec).unwrap_err();
    assert!(e.contains("weld") && e.contains("NPS"), "{e}");
}

#[test]
fn a_reducer_must_match_the_pipe_at_each_end() {
    // An explicit reducer size may not disagree with the pipe it is welded to.
    let spec: Spec = serde_json::from_value(json!({"name": "R", "segments": [
        {"a": [0, 0, 0], "b": [300, 0, 0], "nps": 2.0},
        {"a": [300, 0, 0], "b": [376.2, 0, 0], "nps": 2.0},
        {"a": [376.2, 0, 0], "b": [600, 0, 0], "nps": 1.0}
    ], "fittings": [{"kind": "reducer", "large": [300, 0, 0], "small": [376.2, 0, 0], "nps": 1.5, "nps_small": 1.0}]}))
    .unwrap();
    let e = rp::plan(&spec).unwrap_err();
    assert!(e.contains("reducer") && e.contains("NPS 1.5") && e.contains("NPS 2"), "{e}");
}

#[test]
fn a_fitting_that_meets_a_pipe_at_an_angle_is_refused() {
    // A valve whose port sits on the end of a pipe at 90 deg: no elbow covers the junction, so
    // it is refused instead of welding two perpendicular ports.
    let spec: Spec = serde_json::from_value(json!({"name": "J", "segments": [
        {"a": [0, 0, 0], "b": [300, 0, 0], "nps": 1.0},
        {"a": [300, 0, 0], "b": [300, 200, 0], "nps": 1.0}
    ], "fittings": [{"kind": "valve", "lib": "v", "a": [300, 0, 0], "b": [300, 200, 0], "up": [0, 0, 1]}],
       "valves": {"v": {"step": "/tmp/valve.step", "port_a": [0, 0, 0], "port_b": [0, 200, 0], "up": [0, 0, 1]}}}))
    .unwrap();
    let e = rp::plan(&spec).unwrap_err();
    assert!(e.contains("fitting") && e.contains("angle"), "{e}");
}

#[test]
fn a_carried_move_of_the_anchor_moves_the_anchor_too() {
    let spec: Spec = serde_json::from_value(json!({"name": "A", "nps": 1.0, "start": [0, 0, 0],
                                                      "points": [[0, 0, 0], [1000, 0, 0], [1000, 500, 0]]}))
    .unwrap();
    let (after, moved) = rp::move_point(&spec, [0.0, 0.0, 0.0], [10.0, 0.0, 0.0], true).unwrap();
    assert_eq!(moved, 3);
    assert_eq!(after.start, Some([10.0, 0.0, 0.0]), "the anchor follows the node it names");
    assert_eq!(rp::nodes(&after).unwrap()[0], [10.0, 0.0, 0.0]);
    // A second carried move from the moved anchor finds it.
    let (again, moved) = rp::move_point(&after, [1010.0, 0.0, 0.0], [0.0, 10.0, 0.0], true).unwrap();
    assert_eq!(moved, 2, "the corner and the point beyond it");
    assert_eq!(again.start, Some([10.0, 0.0, 0.0]), "the anchor stays where the first move put it");
}

#[test]
fn a_sloped_corner_takes_a_bend_of_its_own_angle() {
    // A 1 in Sch 40 line whose second leg turns 92.86 deg: the bend is cut to the turn it makes.
    let a: f64 = 92.86_f64.to_radians();
    let corner = [500.0, 0.0, 0.0];
    let end = [corner[0] + 400.0 * a.cos(), 400.0 * a.sin(), 0.0];
    let mut s = Session::default();
    let r = run(&mut s, "route.create", json!({"name": "B", "nps": 1.0, "schedule": "40", "points": [[0, 0, 0], corner, end]}));
    let t = 38.1 * (a / 2.0).tan();
    let cut: Vec<f64> = r["cut_list"].as_array().unwrap().iter().map(|c| f(&c["length"])).collect();
    assert!((cut[0] - (500.0 - t)).abs() < 1e-4, "{cut:?}");
    assert!((cut[1] - (400.0 - t)).abs() < 1e-4, "{cut:?}");
    // The bend is a ring swept through its own angle on the 38.1 mm radius (Pappus).
    let ring = ring_area(33.401, 33.401 - 2.0 * 3.378);
    assert!(rel(volume(&mut s, "B_elbow_1"), ring * a * 38.1) < 2e-3);
    let got = run(&mut s, "route.get", json!({"route": "B"}));
    let e = got["pieces"].as_array().unwrap().iter().find(|p| p["kind"] == "elbow").unwrap();
    assert!((f(&e["angle"]) - 92.86).abs() < 1e-4, "{e}");
    let ports = e["ports"].as_array().unwrap();
    assert!((f(&ports[0][0]) - (500.0 - t)).abs() < 1e-6);
    assert_eq!(r["free_dof"], json!([]));
}

#[test]
fn a_copper_acr_route_builds_its_own_tube_and_bend() {
    // ASTM B280 ACR 1-1/8 in: 28.575 mm OD, 1.0 mm wall; EN 1254 bend R 17.0 mm.
    let mut s = Session::default();
    let r = run(&mut s, "route.create", json!({"name": "C", "nps": 1.125, "schedule": "ACR", "points": [[0, 0, 0], [500, 0, 0], [500, 400, 0]]}));
    let cut: Vec<f64> = r["cut_list"].as_array().unwrap().iter().map(|c| f(&c["length"])).collect();
    assert_eq!(cut, vec![500.0 - 17.0, 400.0 - 17.0]);
    let ring = ring_area(28.575, 28.575 - 2.0);
    assert!(rel(volume(&mut s, "C_pipe_1_125_1"), ring * 483.0) < 1e-3);
    assert!(rel(volume(&mut s, "C_elbow_1"), ring * PI / 2.0 * 17.0) < 2e-3);
}

#[test]
fn a_reducer_fitted_onto_a_tee_outlet_records_its_overlap() {
    // A 2 in tee whose left run outlet carries a 2 x 1 in reducer set 33.5 mm into the tee. The
    // short edge is given both ways round: its ends are the tee and the reducer's large end.
    for (a, b) in [([0, 0, 0], [-30, 0, 0]), ([-30, 0, 0], [0, 0, 0])] {
        let spec = json!({"name": "O", "schedule": "10S", "segments": [
            {"a": [0, 0, 0], "b": [0, 200, 0], "nps": 2.0},
            {"a": [0, 0, 0], "b": [300, 0, 0], "nps": 2.0},
            {"a": a, "b": b, "nps": 2.0},
            {"a": [-30, 0, 0], "b": [-106.2, 0, 0], "nps": 2.0},
            {"a": [-106.2, 0, 0], "b": [-300, 0, 0], "nps": 1.0}
        ], "fittings": [{"kind": "reducer", "large": [-30, 0, 0], "small": [-106.2, 0, 0], "nps": 2.0,
                         "nps_small": 1.0, "joint": [-63.5, 0, 0]}]});
        let pieces = rp::plan(&serde_json::from_value(spec).unwrap()).unwrap();
        let red = pieces.iter().find(|p| p.kind == Kind::Reducer).unwrap();
        assert!((red.overlap.unwrap() - 33.5).abs() < 1e-6, "{:?}", red.overlap);
        assert!(red.ports.iter().any(|p| (p[0] + 63.5).abs() < 1e-6), "{:?}", red.ports);
        let l: Vec<f64> = lengths(&pieces).into_iter().map(|x| x.1).collect();
        assert_eq!(l.len(), 3, "{l:?}");
        assert!(l.contains(&(200.0 - 63.5)) && l.contains(&(300.0 - 63.5)) && l.contains(&(300.0 - 106.2)), "{l:?}");
        // The reducer is held to the tee at the outlet inside its body.
        let (welds, unjoined) = rp::welds(&pieces);
        assert_eq!(unjoined, Vec::<usize>::new());
        assert_eq!(welds.len(), pieces.len() - 1);
    }
}

// ---- the commands ---------------------------------------------------------------------------

/// An L of 1 in Sch 40: 500 mm along X, then 400 mm along Y.
fn l_route() -> Value {
    json!({"name": "L", "nps": 1.0, "points": [[0, 0, 0], [500, 0, 0], [500, 400, 0]]})
}

fn ring_area(od: f64, id: f64) -> f64 {
    PI / 4.0 * (od * od - id * id)
}

fn volume(s: &mut Session, body: &str) -> f64 {
    let m = run(s, "inspect.measure", json!({}));
    m["bodies"].as_array().unwrap().iter().find(|b| b["name"] == body).map(|b| f(&b["volume_mm3"])).unwrap_or_else(|| panic!("no body {body}"))
}

#[test]
fn create_builds_pipes_and_elbows_welded_and_held_and_keeps_the_route() {
    let mut s = Session::default();
    let r = run(&mut s, "route.create", l_route());
    assert_eq!(r["pieces"].as_array().unwrap().len(), 3);
    let cut: Vec<f64> = r["cut_list"].as_array().unwrap().iter().map(|c| f(&c["length"])).collect();
    assert_eq!(cut, vec![500.0 - 38.1, 400.0 - 38.1]);
    assert_eq!(r["welds"], 2);
    assert_eq!(r["free_dof"], json!([]), "every piece held");
    assert_eq!(r["unjoined"], json!([]));
    // Measured volumes: the pipes are rings times their length, the elbow a ring swept a quarter
    // turn on its 38.1 mm radius (Pappus).
    let ring = ring_area(33.401, 33.401 - 2.0 * 3.378);
    assert!(rel(volume(&mut s, "L_pipe_1_1"), ring * 461.9) < 1e-3);
    assert!(rel(volume(&mut s, "L_pipe_1_2"), ring * 361.9) < 1e-3);
    assert!(rel(volume(&mut s, "L_elbow_1"), ring * PI / 2.0 * 38.1) < 2e-3);
    // The elbow sits at the corner: its box reaches the corner's outside and both pipe ends.
    let m = run(&mut s, "inspect.measure", json!({}));
    let e = m["bodies"].as_array().unwrap().iter().find(|b| b["name"] == "L_elbow_1").unwrap();
    let (lo, hi) = (&e["bbox"]["min"], &e["bbox"]["max"]);
    assert!((f(&lo[0]) - 461.9).abs() < 1e-3 && (f(&hi[0]) - (500.0 + 33.401 / 2.0)).abs() < 1e-3, "{e}");
    assert!((f(&lo[1]) + 33.401 / 2.0).abs() < 1e-3 && (f(&hi[1]) - 38.1).abs() < 1e-3, "{e}");
    let routes = run(&mut s, "route.list", json!({}));
    assert_eq!(routes["routes"][0]["name"], "L");
    assert_eq!(routes["routes"][0]["pieces"], 3);
    // One undo step takes the whole route away.
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(run(&mut s, "route.list", json!({}))["routes"], json!([]));
}

#[test]
fn a_sch_10s_route_reports_its_schedule_and_its_b36_19_wall() {
    let mut s = Session::default();
    let r = run(&mut s, "route.create", json!({"name": "S", "nps": 1.5, "schedule": "10S", "points": [[0, 0, 0], [500, 0, 0], [500, 400, 0]]}));
    assert_eq!(r["cut_list"][0]["schedule"], "10S");
    let m = run(&mut s, "inspect.measure", json!({}));
    let pipe = m["bodies"].as_array().unwrap().iter().find(|b| b["name"] == "S_pipe_1_5_1").unwrap();
    // 48.26 mm OD, 2.77 mm Sch 10S wall, 442.8 mm cut.
    let ring: f64 = PI / 4.0 * (48.26 * 48.26 - (48.26_f64 - 2.0 * 2.77).powi(2));
    assert!(rel(f(&pipe["volume_mm3"]), ring * 442.8) < 1e-3, "{pipe}");
}

#[test]
fn moving_a_point_rebuilds_the_route_with_the_new_cut_lengths() {
    let mut s = Session::default();
    run(&mut s, "route.create", l_route());
    let r = run(&mut s, "route.move_point", json!({"route": "L", "at": [500, 400, 0], "delta": [0, 100, 0]}));
    assert_eq!(r["moved_points"], 1);
    let cut: Vec<f64> = r["cut_list"].as_array().unwrap().iter().map(|c| f(&c["length"])).collect();
    assert_eq!(cut, vec![461.9, 461.9]);
    // The corner, carrying the leg beyond it.
    let r = run(&mut s, "route.move_point", json!({"route": "L", "at": [500, 0, 0], "to": [700, 0, 0]}));
    assert_eq!(r["moved_points"], 2);
    let got = run(&mut s, "route.get", json!({"route": "L"}));
    assert_eq!(got["points"], json!([[0.0, 0.0, 0.0], [700.0, 0.0, 0.0], [700.0, 500.0, 0.0]]));
    assert!(rel(volume(&mut s, "L_pipe_1_1"), ring_area(33.401, 26.645) * 661.9) < 1e-3);
    assert_eq!(r["free_dof"], json!([]));
    let comps = run(&mut s, "component.list", json!({"compact": true}));
    assert_eq!(comps["components"].as_array().unwrap().len(), 1 + 1 + 3, "the root, the route and its three pieces, nothing left over");
}

#[test]
fn a_renamed_route_keeps_its_component_name_in_the_list_and_after_an_edit() {
    let mut s = Session::default();
    run(&mut s, "route.create", l_route());
    let listed = run(&mut s, "route.list", json!({}));
    let component = listed["routes"][0]["component"].clone();
    run(&mut s, "component.rename", json!({"component": component, "name": "M"}));
    // The component is the route's identity: the list and the details report it, not the stale
    // name stored in the spec.
    assert_eq!(run(&mut s, "route.list", json!({}))["routes"][0]["name"], "M");
    assert_eq!(run(&mut s, "route.get", json!({"route": "M"}))["route"], "M");
    run(&mut s, "route.edit", json!({"route": "M", "schedule": "80"}));
    assert_eq!(run(&mut s, "route.list", json!({}))["routes"][0]["name"], "M", "an edit keeps the component's name");
}

#[test]
fn a_schedule_en_cap_takes_the_pipe_tube_diameter() {
    // EN 10253-4 2.5 in tube is 63.5 x 1.65 mm, not the B36.10 73.03 mm pipe; the cap must
    // match the pipe it closes.
    let mut s = Session::default();
    run(
        &mut s,
        "route.create",
        json!({"name": "E", "nps": 2.5, "schedule": "EN", "points": [[0, 0, 0], [500, 0, 0]],
                                       "fittings": [{"kind": "cap", "at": [500, 0, 0]}]}),
    );
    let m = run(&mut s, "inspect.measure", json!({}));
    let cap = m["bodies"].as_array().unwrap().iter().find(|b| b["name"] == "E_cap_1").unwrap();
    let (lo, hi) = (&cap["bbox"]["min"], &cap["bbox"]["max"]);
    assert!((f(&hi[1]) - f(&lo[1]) - 63.5).abs() < 1e-3, "the cap's diameter is the tube's: {cap}");
    assert!((f(&hi[2]) - f(&lo[2]) - 63.5).abs() < 1e-3, "the cap's diameter is the tube's: {cap}");
}

#[test]
fn edit_changes_the_schedule_and_elbow_and_delete_removes_the_route() {
    let mut s = Session::default();
    run(&mut s, "route.create", l_route());
    run(&mut s, "route.edit", json!({"route": "L", "schedule": "80", "elbow": "short_radius"}));
    let ring80 = ring_area(33.401, 33.401 - 2.0 * 4.55);
    assert!(rel(volume(&mut s, "L_pipe_1_1"), ring80 * (500.0 - 25.4)) < 1e-3);
    let got = run(&mut s, "route.get", json!({"route": "L"}));
    assert_eq!(got["spec"]["schedule"], "80");
    assert_eq!(got["cut_list"][0]["schedule"], "80");
    let r = run(&mut s, "route.delete", json!({"route": "L"}));
    assert_eq!(r["deleted"].as_array().unwrap().len(), 4);
    assert_eq!(run(&mut s, "route.list", json!({}))["routes"], json!([]));
}

#[test]
fn bad_calls_leave_the_design_as_it_was() {
    let mut s = Session::default();
    run(&mut s, "route.create", l_route());
    let before = s.doc.clone();
    for (id, p) in [
        ("route.create", l_route()),
        ("route.create", json!({"name": "X", "nps": 1.0, "points": [[0, 0, 0], [500, 0, 0]], "frame": [2, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0]})),
        ("route.move_point", json!({"route": "L", "at": [1, 2, 3], "delta": [0, 1, 0]})),
        ("route.move_point", json!({"route": "L", "at": [500, 0, 0], "delta": [0, 0, 0]})),
        ("route.move_point", json!({"route": "L", "at": [500, 400, 0], "delta": [0, -390, 0]})),
        ("route.edit", json!({"route": "L", "nps": 9})),
        ("route.edit", json!({"route": "nope"})),
        ("route.get", json!({})),
        ("route.create", json!({"name": "Y", "nps": 1.0, "points": "no"})),
    ] {
        assert!(s.execute(id, &p).is_err(), "{id} {p}");
        assert_eq!(s.doc, before, "{id} {p} changed the design");
    }
}

#[test]
fn a_framed_route_is_built_where_the_frame_puts_it() {
    let mut s = Session::default();
    let mut p = l_route();
    // A quarter turn about Z, then 1000 mm along X.
    p["frame"] = json!([0, -1, 0, 1000, 1, 0, 0, 0, 0, 0, 1, 0]);
    run(&mut s, "route.create", p);
    let got = run(&mut s, "route.get", json!({"route": "L"}));
    assert_eq!(got["points"][2], json!([600.0, 500.0, 0.0]));
}

#[test]
fn the_library_lists_every_size_with_its_fittings() {
    let mut s = Session::default();
    let lib = run(&mut s, "route.fittings", json!({}));
    let p2 = lib["pipes"].as_array().unwrap().iter().find(|p| p["nps"] == 2.0).unwrap();
    assert_eq!((f(&p2["od"]), f(&p2["elbow_long_radius"]), f(&p2["elbow_short_radius"])), (60.325, 76.2, 50.8));
    assert_eq!(f(&p2["wall"]["10S"]), 2.77);
    let ph = lib["pipes"].as_array().unwrap().iter().find(|p| p["nps"] == 0.5).unwrap();
    assert_eq!(f(&ph["wall"]["10S"]), 2.11);
    let p125 = lib["pipes"].as_array().unwrap().iter().find(|p| p["nps"] == 1.25).unwrap();
    assert_eq!((f(&p125["elbow_long_radius"]), f(&p125["tee"])), (47.6, 47.6));
    let red = lib["reducers"].as_array().unwrap();
    assert!(red.iter().any(|r| f(&r["nps"]) == 1.25 && f(&r["nps_small"]) == 1.0 && f(&r["h"]) == 50.8));
    assert!(red.iter().any(|r| f(&r["nps"]) == 3.0 && f(&r["nps_small"]) == 2.0 && f(&r["h"]) == 88.9));
    assert_eq!(lib["schedules"], json!(["10S", "40", "40S", "80", "ACR", "EN"]));
    let cu28 = lib["pipes"].as_array().unwrap().iter().find(|p| f(&p["nps"]) == 1.125).unwrap();
    assert_eq!((f(&cu28["od"]), f(&cu28["wall"]["ACR"]), f(&cu28["elbow_long_radius"])), (28.575, 1.0, 17.0));
    let cu22 = lib["pipes"].as_array().unwrap().iter().find(|p| f(&p["nps"]) == 0.875).unwrap();
    assert_eq!((f(&cu22["od"]), f(&cu22["elbow_long_radius"])), (22.225, 13.0));
    let tube = lib["pipes"].as_array().unwrap().iter().find(|p| f(&p["nps"]) == 2.5).unwrap();
    assert_eq!((f(&tube["od_en"]), f(&tube["wall"]["EN"])), (63.5, 1.65));
}

#[test]
fn a_copper_tube_size_is_not_a_steel_pipe_size() {
    use rp::{Schedule, od, wall};
    for s in [Schedule::S10S, Schedule::S40, Schedule::S40S, Schedule::S80] {
        assert_eq!(od(1.125, s), None, "{s:?}");
        assert_eq!(wall(0.875, s), None, "{s:?}");
    }
    assert_eq!(od(1.125, Schedule::Acr), Some(28.575));
    assert_eq!(od(1.0, Schedule::S40), Some(33.401));
}
