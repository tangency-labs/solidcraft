//! The Extrude dialog's Thin Extrude (#721): a picked profile's outline becomes the wall's
//! curves, and editing a thin extrude keeps it thin.
use crate::scenario::Harness;
use serde_json::json;
use solidcraft_engine::Sel;
use solidcraft_engine::doc::{FeatureKind, ThinSide};

fn rectangle(h: &mut Harness) -> u64 {
    h.app.run("sketch.create", json!({"plane":"XY","name":"Base"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[40,20]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.app.session.doc.find_feature("Base").unwrap().id
}

fn volume(h: &Harness) -> f64 {
    h.app.session.world_state().unwrap().bodies.iter().map(|b| solidcraft_engine::doc::kernel::measure(&b.body).unwrap().volume).sum()
}

#[test]
fn a_thin_extrude_of_a_picked_profile_follows_its_outline() {
    let mut h = Harness::new();
    let sketch = rectangle(&mut h);
    h.step(&json!({"call":"ui.start","params":{"command":"solid.extrude"}})).unwrap();
    h.frames(3);
    h.app.dialog.as_mut().unwrap().inputs[0].items = vec![Sel::Profile { sketch, index: 0 }];
    h.step(&json!({"widget":"Thin Extrude"})).unwrap();
    h.frames(3);
    assert!(h.widget("Wall Thickness").is_some(), "the wall rows show once Thin is on");
    h.step(&json!({"widget":"OK"})).unwrap();
    assert!(h.app.dialog.is_none(), "{:?}", h.app.session.log);
    let f = h.app.session.doc.features.last().unwrap();
    let FeatureKind::Extrude { thin: Some(thin), .. } = &f.kind else { panic!("a thin extrude: {:?}", f.kind) };
    assert_eq!(thin.curves.len(), 4);
    assert_eq!(thin.side, ThinSide::Center);
    // A 2 mm wall centred on the 40 × 20 outline, 10 mm high.
    assert!((volume(&h) - (42.0 * 22.0 - 38.0 * 18.0) * 10.0).abs() < 1e-6, "{}", volume(&h));
}

#[test]
fn editing_a_thin_extrude_keeps_its_wall() {
    let mut h = Harness::new();
    rectangle(&mut h);
    h.app
        .run(
            "solid.extrude",
            json!({"sketch":"Base","thin":true,"thickness":"3 mm","side":"two_sides","thickness2":"1 mm","distance":"5 mm","name":"Wall"}),
        )
        .unwrap();
    h.frames(4);
    let before = h.app.session.doc.find_feature("Wall").unwrap().clone();
    let v = volume(&h);
    h.step(&json!({"call":"ui.editFeature","params":{"feature":"Wall"}})).unwrap();
    h.frames(8);
    assert!(h.widget("Thickness Outside").is_some());
    h.step(&json!({"widget":"OK"})).unwrap();
    assert!(h.app.dialog.is_none(), "{:?}", h.app.session.log);
    let after = h.app.session.doc.find_feature("Wall").unwrap();
    assert_eq!(after.kind, before.kind);
    assert_eq!(after.param_names, before.param_names);
    assert!((volume(&h) - v).abs() < 1e-9);
}

#[test]
fn the_thin_rows_are_translated() {
    let cases = [
        ("ja", "薄肉押し出し", "壁の位置", "外側の厚さ"),
        ("de", "Dünnwandig extrudieren", "Wandlage", "Dicke außen"),
        ("uk", "Тонке видавлювання", "Розташування стінки", "Товщина назовні"),
    ];
    for (lang, thin, location, outside) in cases {
        let mut h = Harness::new();
        h.app.preferences.language = lang.into();
        rectangle(&mut h);
        h.app
            .run(
                "solid.extrude",
                json!({"sketch":"Base","thin":true,"thickness":"3 mm","side":"two_sides","thickness2":"1 mm","distance":"5 mm","name":"Wall"}),
            )
            .unwrap();
        h.step(&json!({"call":"ui.editFeature","params":{"feature":"Wall"}})).unwrap();
        h.frames(8);
        for word in [thin, location, outside] {
            assert!(h.widget(word).is_some(), "{lang}: {word:?} must be on screen");
        }
        assert!(h.widget("Wall Location").is_none(), "{lang}: must not fall back to English");
    }
}
