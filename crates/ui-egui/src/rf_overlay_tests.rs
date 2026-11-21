use serde_json::json;
use solidcraft_engine::Session;

use super::*;
use crate::Services;

/// A room 4 m × 3 m with 150 mm walls, 2.5 m high, and a ceiling over it.
fn room() -> SolveApp {
    let mut app = SolveApp::new(Session::default(), Services::default());
    for (name, corner, size) in [
        ("South", [-150, -150, 0], [4300, 150, 2500]),
        ("North", [-150, 3000, 0], [4300, 150, 2500]),
        ("West", [-150, 0, 0], [150, 3000, 2500]),
        ("East", [4000, 0, 0], [150, 3000, 2500]),
        ("Ceiling", [-150, -150, 2500], [4300, 3300, 100]),
    ] {
        app.run(
            "solid.box",
            json!({"body_name": name, "corner": corner, "length": size[0], "width": size[1], "height": size[2], "operation": "new"}),
        )
        .unwrap();
    }
    app
}

#[test]
fn the_ribbon_button_turns_the_heatmap_on_with_a_cut_above_it_and_off_again() {
    let mut app = room();
    assert!(gpu_image(&app).is_none() && clip(&app).is_none());
    toggle(&mut app);
    let image = gpu_image(&app).expect("a heatmap");
    let o = app.session.rf_coverage.clone().unwrap();
    assert_eq!(image.size, [o.grid.columns as u32, o.grid.rows as u32]);
    assert_eq!(image.rgba.len(), o.rgba.len());
    // The image lies just under the slice, and the view is cut at the slice, so the ceiling
    // doesn't hide it.
    assert!(image.corners.iter().all(|c| (f64::from(c[2]) - (o.height - BELOW_MM)).abs() < 1e-3));
    let (origin, normal) = clip(&app).unwrap();
    assert_eq!((origin.z, normal.z), (o.height, 1.0));
    assert!(crate::viewport::section_planes(&app).iter().any(|p| p.normal.z == 1.0));
    // The image's first row is the grid's top: the engine's last.
    let w = o.grid.columns * 4;
    assert_eq!(&image.rgba[..w], &o.rgba[o.rgba.len() - w..]);
    // A section the user made wins over the cut.
    app.run("inspect.section", json!({"plane": "XZ", "offset": 1})).unwrap();
    assert!(clip(&app).is_none());
    app.run("inspect.section", json!({"clear": true})).unwrap();
    toggle(&mut app);
    assert!(app.session.rf_coverage.is_none() && gpu_image(&app).is_none());
}

#[test]
fn the_card_changes_the_band_device_and_height_and_adds_and_clears() {
    let mut app = room();
    toggle(&mut app);
    let d = details(&app).unwrap();
    assert_eq!(d.settings.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), ["device", "band", "height", "slice", "walls"]);
    assert_eq!(d.key.len(), solidcraft_engine::rf_coverage::SCALE.len());
    // The key shows the share of the floor at each level or better: falling, 100% at the end.
    let shares: Vec<f64> = d.key.iter().map(|k| k.value.trim_end_matches('%').parse().unwrap()).collect();
    assert!(shares.windows(2).take(shares.len() - 2).all(|w| w[0] <= w[1]), "{shares:?}");
    dispatch(&mut app, DetailEvent::Change("band".into(), DetailValue::Choice { selected: 2, options: Vec::new() }));
    assert_eq!(app.session.rf_coverage.as_ref().unwrap().band.name(), "6");
    let custom = DEVICES.len();
    dispatch(&mut app, DetailEvent::Change("device".into(), DetailValue::Choice { selected: custom, options: Vec::new() }));
    assert_eq!(app.session.rf_coverage.as_ref().unwrap().emitters[0].eirp(solidcraft_engine::doc::rf::Band::G6), Some(20.0));
    let units = app.session.doc.units.clone();
    dispatch(&mut app, DetailEvent::Change("height".into(), DetailValue::Length { value: 2400.0, units, decimals: 1 }));
    assert_eq!(app.session.rf_coverage.as_ref().unwrap().emitters[0].height, 2400.0);
    dispatch(&mut app, DetailEvent::Action("add".into()));
    assert_eq!(app.session.rf_coverage.as_ref().unwrap().emitters.len(), 2);
    dispatch(&mut app, DetailEvent::Action("clear".into()));
    assert!(app.session.rf_coverage.is_none());
}

#[test]
fn moving_an_emitter_keeps_the_full_settings_for_the_release() {
    let mut app = room();
    toggle(&mut app);
    let o = app.session.rf_coverage.clone().unwrap();
    let p = moved(&o, 0, [1000.0, 1000.0]).unwrap();
    assert_eq!(p["emitters"][0]["at"], json!([1000.0, 1000.0]));
    assert!(moved(&o, 5, [0.0, 0.0]).is_none());
    // An `at` with a height keeps the height when moved.
    let mut with_z = (*o).clone();
    with_z.params["emitters"][0] = json!({"at": [1, 2, 2400]});
    assert_eq!(moved(&with_z, 0, [5.0, 6.0]).unwrap()["emitters"][0], json!({"at": [5.0, 6.0], "height": 2400.0}));
}
