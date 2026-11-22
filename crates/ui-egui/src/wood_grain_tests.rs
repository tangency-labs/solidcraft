use super::*;
use crate::dialogs::{Dialog, apply_commands};
use crate::scenario::Harness;
use solidcraft_engine::geom::prelude::*;

fn fixture() -> Harness {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":100,"width":30,"height":10,"corner":[0,0,0],"body_name":"Board"})).unwrap();
    h
}

#[test]
fn wood_grain_dialog_picks_a_body_and_direction_and_previews_without_editing() {
    let mut h = fixture();
    assert!(!h.app.ui.show_grain);
    h.app.session.selection = vec![Sel::Body { name: "Board".into() }];
    h.app.start("wood.grain");
    let before = h.app.session.doc.clone();
    let mut d = h.app.dialog.clone().unwrap();
    let Kind::WoodGrain(grain) = &mut d.kind else { panic!("grain dialog") };
    grain.mode = 1;
    let commands = apply_commands(&h.app, &d).unwrap();
    assert_eq!(commands, vec![("wood.grain".into(), json!({"body":"Board","direction":"length"}))]);
    h.app.dialog = Some(d);
    assert_eq!(preview(&h.app), Some(commands[0].1.clone()));
    let without = {
        let dialog = h.app.dialog.take();
        let count = crate::viewport::build_scene(&h.app).lines.len();
        h.app.dialog = dialog;
        count
    };
    assert!(crate::viewport::build_scene(&h.app).lines.len() > without);
    assert_eq!(h.app.session.doc, before, "preview leaves the document untouched");
    h.app.dialog = None;
    h.app.run(&commands[0].0, commands[0].1.clone()).unwrap();
    h.app.run("view.grain", json!({"on":true})).unwrap();
    assert!(h.app.ui.show_grain);
    assert!(crate::toolbar::shows_pressed(&h.app, "view.grain"));
    assert!(crate::viewport::build_scene(&h.app).lines.len() > without);
    h.app.run("view.grain", json!({})).unwrap();
    assert!(!h.app.ui.show_grain);
    assert_eq!(crate::viewport::build_scene(&h.app).lines.len(), without);
}

#[test]
fn a_grain_guide_is_parallel_to_the_grain_and_stays_on_the_box_faces() {
    let h = fixture();
    let state = h.app.session.world_state().unwrap();
    let body = state.body("Board").unwrap();
    let segments = crate::grain_guides::lines(&body.mesh(), solidcore_record::Vec3::X);
    assert!(!segments.is_empty());
    let mut top: Vec<f64> = segments.iter().filter(|(a, b)| (a.z - 10.).abs() < 0.001 && (b.z - 10.).abs() < 0.001).map(|(a, _)| a.y).collect();
    top.sort_by(f64::total_cmp);
    top.dedup_by(|a, b| (*a - *b).abs() < 1e-7);
    assert_eq!(top, [7.5, 15., 22.5], "three interior guides across the 30 mm face");
    for (a, b) in segments {
        assert!((b - a).normalized().unwrap().dot(solidcore_record::Vec3::X).abs() > 1. - 1e-8);
        for p in [a, b] {
            assert!(p.x >= -0.001 && p.x <= 100.001 && p.y >= -0.001 && p.y <= 30.001 && p.z >= -0.001 && p.z <= 10.001, "{p:?}");
        }
    }
}

#[test]
fn grain_dialog_refuses_a_direction_picked_on_another_body() {
    let h = fixture();
    let (kind, mut inputs) = start(&h.app, "wood.grain").unwrap();
    inputs[0].items.push(Sel::Body { name: "Board".into() });
    inputs[1].items.push(Sel::Edge { body: "Other".into(), index: 0, point: solidcore_record::Vec3::ZERO });
    let d = Dialog::new(kind, inputs);
    assert!(apply_commands(&h.app, &d).unwrap_err().contains("selected body"));
}

#[test]
fn grain_scenario_sets_the_direction_and_toggles_visible_guides() {
    if let Ok(dir) = std::env::var("SOLIDCRAFT_SCENARIO_SHOTS") {
        std::fs::create_dir_all(dir).unwrap();
    }
    let mut h = Harness::new();
    h.run(&serde_json::from_str(include_str!("../tests/scenarios/wood_grain.json")).unwrap()).unwrap();
    assert!(!h.app.ui.show_grain);
    assert_eq!(h.app.session.doc.wood_grain["Board"].direction, Some([1., 0., 0.]));
}
