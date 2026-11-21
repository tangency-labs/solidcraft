use serde_json::{Value, json};

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn wall(s: &mut Session, name: &str, corner: [f64; 3], size: [f64; 3]) {
    run(s, "solid.box", json!({"body_name": name, "corner": corner, "length": size[0], "width": size[1], "height": size[2], "operation": "new"}));
}

/// Two rooms 4 m × 3 m side by side, 2.5 m high: 150 mm walls round them (Concrete, a design
/// material with the concrete preset) and a 90 mm partition between them with no material.
fn two_rooms() -> Session {
    let mut s = Session::default();
    wall(&mut s, "South", [-150.0, -150.0, 0.0], [8390.0, 150.0, 2500.0]);
    wall(&mut s, "North", [-150.0, 3000.0, 0.0], [8390.0, 150.0, 2500.0]);
    wall(&mut s, "West", [-150.0, 0.0, 0.0], [150.0, 3000.0, 2500.0]);
    wall(&mut s, "East", [8090.0, 0.0, 0.0], [150.0, 3000.0, 2500.0]);
    wall(&mut s, "Partition", [4000.0, 0.0, 0.0], [90.0, 3000.0, 2500.0]);
    run(&mut s, "material.create", json!({"name": "Concrete", "density_g_cm3": 2.4, "rf": "concrete"}));
    run(&mut s, "material.assign", json!({"bodies": ["South", "North", "West", "East"], "material": "Concrete"}));
    s
}

fn ap() -> Value {
    json!([{"name": "Hall", "at": [2000, 1500], "eirp_dbm": 30}])
}

/// RSSI with no walls: 30 − FSPL(1 m, 5500 MHz) − 22·log10(d), FSPL(1 m) = 20·log10(5500) − 27.55.
fn free(d_m: f64) -> f64 {
    30.0 - (20.0 * 5500f64.log10() - 27.55) - 22.0 * d_m.log10()
}

#[test]
fn two_rooms_give_the_expected_signal_and_stats() {
    let mut s = two_rooms();
    let undo = s.undo.len();
    let out = run(
        &mut s,
        "inspect.rf_coverage",
        json!({"emitters": ap(), "band": "5", "points": [{"name": "Near", "at": [3000, 1500]}, {"name": "Next room", "at": [6000, 1500]}]}),
    );
    // Four concrete walls and the guessed drywall partition.
    assert_eq!(out["walls"]["bodies"], 5, "{out}");
    assert_eq!(out["walls"]["materials"], json!({"Concrete": 4, "guessed by thickness": 1}));
    let near = out["points"][0]["rssi_dbm"]["5"].as_f64().unwrap();
    let next = out["points"][1]["rssi_dbm"]["5"].as_f64().unwrap();
    assert!((near - free(1.0)).abs() < 0.06, "{near}");
    // 4 m away behind the partition: 4 dB of drywall at 5 GHz.
    assert!((next - (free(4.0) - 4.0)).abs() < 0.06, "{next}");
    assert_eq!(out["points"][1]["from"]["5"], "Hall");
    // The floor is what the walls enclose: 24 m² less the cells their edges touch.
    assert_eq!(out["grid"]["region"], "enclosed");
    let floor = out["grid"]["floor_m2"].as_f64().unwrap();
    assert!((20.0..24.0).contains(&floor), "{floor}");
    let st = &out["stats"][0];
    assert_eq!(st["band"], "5");
    assert_eq!(st["pct_at_least_60"], 100.0);
    assert!(st["p5_dbm"].as_f64().unwrap() <= st["median_dbm"].as_f64().unwrap());
    // The weakest cell is a far corner of the next room: about 6.3 m and the partition.
    let min = st["min_dbm"].as_f64().unwrap();
    assert!((min - (free(6.09f64.hypot(1.46)) - 4.0)).abs() < 1.0, "{min}");
    // The reply is small: no grid values unless asked.
    assert!(out["grid"].get("values").is_none());
    // View only: no undo step, the heatmap is on.
    assert_eq!(s.undo.len(), undo);
    let overlay = s.rf_coverage.clone().unwrap();
    assert_eq!(overlay.rgba.len(), overlay.grid.columns * overlay.grid.rows * 4);
    assert_eq!(overlay.height, 1000.0);
    // Moving the emitter reuses the walls.
    let moved = run(&mut s, "inspect.rf_coverage", json!({"emitters": [{"at": [6000, 1500], "eirp_dbm": 30}], "band": "5"}));
    assert_eq!(moved["walls_reused"], true);
    assert!(s.rf_coverage.as_ref().unwrap().version > overlay.version);
    run(&mut s, "inspect.rf_coverage", json!({"clear": true}));
    assert!(s.rf_coverage.is_none());
}

#[test]
fn a_concrete_partition_costs_more_and_thick_walls_without_material_are_concrete() {
    let mut s = two_rooms();
    let p = json!({"emitters": ap(), "band": "all", "points": [{"name": "Next room", "at": [6000, 1500]}]});
    let drywall = run(&mut s, "inspect.rf_coverage", p.clone());
    // The partition made 200 mm thick, and no material anywhere: every wall is guessed concrete.
    let mut s = Session::default();
    wall(&mut s, "South", [-150.0, -150.0, 0.0], [8390.0, 150.0, 2500.0]);
    wall(&mut s, "North", [-150.0, 3000.0, 0.0], [8390.0, 150.0, 2500.0]);
    wall(&mut s, "West", [-150.0, 0.0, 0.0], [150.0, 3000.0, 2500.0]);
    wall(&mut s, "East", [8090.0, 0.0, 0.0], [150.0, 3000.0, 2500.0]);
    wall(&mut s, "Partition", [4000.0, 0.0, 0.0], [200.0, 3000.0, 2500.0]);
    let concrete = run(&mut s, "inspect.rf_coverage", p);
    assert_eq!(concrete["walls"]["materials"], json!({"guessed by thickness": 5}));
    for (band, extra) in [("2.4", 12.0 - 3.0), ("5", 20.0 - 4.0), ("6", 24.0 - 5.0)] {
        let a = drywall["points"][0]["rssi_dbm"][band].as_f64().unwrap();
        let b = concrete["points"][0]["rssi_dbm"][band].as_f64().unwrap();
        // 110 mm further through the thicker wall changes nothing: losses are per crossing.
        assert!((a - b - extra).abs() < 0.11, "{band}: {a} {b}");
    }
    assert_eq!(drywall["stats"].as_array().unwrap().len(), 3);
}

#[test]
fn scope_hidden_bodies_regions_devices_and_png() {
    let mut s = two_rooms();
    // A sphere planter at the slice height: cut by the half-space path, guessed concrete (600 mm).
    run(&mut s, "solid.sphere", json!({"body_name": "Planter", "center": [6000, 2000, 1000], "radius": 300}));
    let out = run(&mut s, "inspect.rf_coverage", json!({"emitters": ap()}));
    assert_eq!(out["walls"]["bodies"], 6, "{out}");
    assert_eq!(out["walls"]["materials"]["guessed by thickness"], 2);
    let out = run(&mut s, "inspect.rf_coverage", json!({"emitters": ap(), "exclude": ["Planter"]}));
    assert_eq!(out["walls"]["bodies"], 5);
    let out = run(&mut s, "inspect.rf_coverage", json!({"emitters": ap(), "bodies": ["Partition"]}));
    assert_eq!(out["walls"]["bodies"], 1);
    // Nothing encloses a lone partition: the floor is every cell not in a wall.
    assert_eq!(out["grid"]["region"], "open");
    // A hidden pane without an RF material is left out; with low-E glass it counts.
    wall(&mut s, "Pane", [1000.0, 2000.0, 0.0], [2000.0, 6.0, 2500.0]);
    run(&mut s, "browser.visibility", json!({"items": ["Pane"], "visible": false}));
    let scope = json!({"emitters": ap(), "bodies": ["Pane"], "points": [{"name": "Behind", "at": [2000, 2800]}]});
    assert!(s.execute("inspect.rf_coverage", &scope).is_err_and(|e| e.to_string().contains("nothing makes walls")));
    run(&mut s, "material.create", json!({"name": "Low-E", "density_g_cm3": 2.5, "rf": "low-e-glass"}));
    run(&mut s, "material.assign", json!({"bodies": ["Pane"], "material": "Low-E"}));
    let out = run(&mut s, "inspect.rf_coverage", scope);
    assert_eq!(out["walls"]["materials"], json!({"Low-E": 1}));
    let behind = out["points"][0]["rssi_dbm"]["5"].as_f64().unwrap();
    assert!((behind - (free(1.3) - 15.0)).abs() < 0.06, "{behind}");
    // Scoped to a body, bodies with an RF material can still count.
    let out = run(&mut s, "inspect.rf_coverage", json!({"emitters": ap(), "bodies": ["Partition"], "rf_materials": true}));
    assert_eq!(out["walls"]["materials"], json!({"Concrete": 4, "Low-E": 1, "guessed by thickness": 1}));
    // A polygon region; a device from the library, capped at 30 dBm on 5 GHz and 27 on 6.
    let png = std::env::temp_dir().join(format!("rf-coverage-{}.png", std::process::id()));
    let out = run(
        &mut s,
        "inspect.rf_coverage",
        json!({"emitters": [{"at": [2000, 1500, 2400], "device": "u7-pro"}], "region": {"polygon": [[0, 0], [4000, 0], [4000, 3000], [0, 3000]]},
               "png": png.to_string_lossy(), "grid": true, "band": "6"}),
    );
    assert_eq!(out["grid"]["region"], "polygon");
    assert_eq!(out["emitters"][0]["eirp_dbm"], json!({"2.4": 27.0, "5": 30.0, "6": 27.0}));
    assert_eq!(out["emitters"][0]["height"], 2400.0);
    assert!(out["grid"]["values"]["6"].as_array().is_some_and(|v| v.iter().any(Value::is_number)));
    let bytes = std::fs::read(&png).unwrap();
    assert!(bytes.starts_with(b"\x89PNG"));
    let _ = std::fs::remove_file(&png);
    // With no emitters: the device library.
    let lib = run(&mut s, "inspect.rf_coverage", json!({}));
    assert!(lib["devices"].as_array().is_some_and(|d| d.iter().any(|d| d["id"] == "udr7")));
}

#[test]
fn hostile_params_are_refused_and_change_nothing() {
    let mut s = two_rooms();
    let doc = s.doc.clone();
    for p in [
        json!({"emitters": [{"at": [0]}]}),
        json!({"emitters": [{"at": [0, 0], "device": "nope"}]}),
        json!({"emitters": [{"at": [0, 0], "device": "udr7", "eirp_dbm": 20}]}),
        json!({"emitters": [{"at": [0, f64::MAX], "eirp_dbm": 20}]}),
        json!({"emitters": [{"at": [0, 0], "eirp_dbm": 1000}]}),
        json!({"emitters": [{"at": [0, 0], "eirp_dbm": {"5": 20}}], "band": "6"}),
        json!({"emitters": ap(), "grid_step": 0}),
        json!({"emitters": ap(), "grid_step": 10, "region": {"polygon": [[0, 0], [1e7, 0], [1e7, 1e7]]}}),
        json!({"emitters": ap(), "exponent": 99}),
        json!({"emitters": ap(), "slice_height": 99999}),
        json!({"emitters": ap(), "region": {"polygon": [[0, 0], [1, 1]]}}),
        json!({"emitters": ap(), "region": {"body": "Nope"}}),
        json!({"emitters": ap(), "png": "/tmp/x.txt"}),
        json!({"emitters": ap(), "surprise": 1}),
        json!({"clear": true, "emitters": ap()}),
        json!({"emitters": "x"}),
    ] {
        assert!(s.execute("inspect.rf_coverage", &p).is_err(), "{p}");
    }
    assert!(std::sync::Arc::ptr_eq(&doc, &s.doc));
}
