use super::*;

fn save(s: &mut Session, patch: Value) {
    s.execute("render.settings.set", &json!({"name":"Studio", "set":patch})).unwrap();
}

#[test]
fn settings_history_preserves_geometry_cache_and_dirty_state() {
    let mut s = Session::default();
    let component = s.execute("component.create", &json!({"name":"Part","grounded":false})).unwrap();
    s.execute("solid.box", &json!({"length":10,"width":20,"height":5})).unwrap();
    s.execute("component.activate", &json!({"component":0})).unwrap();
    s.execute("occurrence.move", &json!({"occurrence":component["occurrence"],"translate":[30,0,0]})).unwrap();
    s.mark_saved();
    let model = s.model.state();
    let world = s.world_state();
    assert!(!Arc::ptr_eq(&model, &world), "exercise the prepared occurrence cache");
    let mesh = model.bodies[0].mesh();
    let undo = s.undo.len();
    s.evaluations = 0;
    save(&mut s, json!({"exposure_ev":2,"backend":"future-backend"}));
    assert!(s.is_dirty());
    assert_eq!(s.undo.len(), undo + 1);
    let doc = s.doc.clone();
    let revision = s.revision;
    save(&mut s, json!({"exposure_ev":2}));
    assert!(Arc::ptr_eq(&doc, &s.doc));
    assert_eq!(s.revision, revision);
    assert_eq!(s.undo.len(), undo + 1);
    let restored = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
    assert_eq!(restored.render, s.doc.render);
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(!s.is_dirty());
    s.execute("render.settings.get", &json!({})).unwrap();
    assert_eq!(s.redo.len(), 1);
    s.execute("edit.redo", &json!({})).unwrap();
    let checkpoint = s.checkpoint();
    save(&mut s, json!({"width":800}));
    s.execute("edit.undo", &json!({})).unwrap();
    let revision = s.revision;
    save(&mut s, json!({"exposure_ev":2}));
    assert_eq!(s.redo.len(), 1, "a no-op edit preserves redo");
    assert_eq!(s.revision, revision);
    s.execute("edit.redo", &json!({})).unwrap();
    s.restore_checkpoint(checkpoint).unwrap();
    assert_eq!(s.doc.render.setups["Studio"].width, 1024);
    assert!(Arc::ptr_eq(&model, &s.model.state()));
    assert!(Arc::ptr_eq(&mesh, &s.model.state().bodies[0].mesh()));
    assert!(Arc::ptr_eq(&world, &s.world_state()));
    assert_eq!(s.evaluations, 0);
    // A geometry change and its history still require normal evaluation.
    s.execute("solid.box", &json!({"length":2,"width":3,"height":4})).unwrap();
    assert!(s.evaluations > 0);
    s.evaluations = 0;
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(s.evaluations > 0);
    s.evaluations = 0;
    s.execute("edit.redo", &json!({})).unwrap();
    assert!(s.evaluations > 0);
    let checkpoint = s.checkpoint();
    let expected = s.doc.clone();
    s.execute("solid.box", &json!({"length":1,"width":1,"height":1})).unwrap();
    s.evaluations = 0;
    s.execute("edit.restore_checkpoint", &json!({"checkpoint":checkpoint})).unwrap();
    assert_eq!(*s.doc, *expected);
    assert!(s.evaluations > 0);
}

#[test]
fn invalid_settings_and_limit_are_atomic() {
    let mut s = Session::default();
    let mut camera_with_typo = serde_json::to_value(solidcraft_render::Camera::default()).unwrap();
    camera_with_typo["fov_degrees"] = json!(45);
    for patch in [
        json!({"width":0}),
        json!({"width":4097}),
        json!({"height":null}),
        json!({"unknown":true}),
        json!({"camera":{"distance":0}}),
        json!({"camera":camera_with_typo}),
        json!({"exposure_ev":33}),
        json!({"environment_intensity":101}),
    ] {
        let doc = s.doc.clone();
        let state = (s.revision, s.undo.len(), s.redo.len(), s.evaluations);
        assert!(s.execute("render.settings.set", &json!({"name":"Studio","set":patch})).is_err());
        assert!(Arc::ptr_eq(&doc, &s.doc));
        assert_eq!(state, (s.revision, s.undo.len(), s.redo.len(), s.evaluations));
    }
    for i in 0..32 {
        s.execute("render.settings.set", &json!({"name":i.to_string(),"set":{}})).unwrap();
    }
    let doc = s.doc.clone();
    assert!(s.execute("render.settings.set", &json!({"name":"overflow","set":{}})).is_err());
    assert!(Arc::ptr_eq(&doc, &s.doc));
    assert!(s.execute("render.settings.get", &json!({"name":"missing"})).is_err());
    assert_eq!(s.evaluations, 0);
}

#[test]
fn settings_history_and_checkpoint_preserve_a_stale_live_drag() {
    let mut s = Session::default();
    for (id, p) in [
        ("sketch.create", json!({"plane":"XY"})),
        ("sketch.rectangle.two_point", json!({"p0":[5,5],"p1":[25,20]})),
        ("sketch.dimension", json!({"entities":["l1"],"value":20})),
        ("sketch.dimension", json!({"entities":["l2"],"value":15})),
        ("sketch.finish", json!({})),
        ("solid.extrude", json!({"distance":5})),
        ("sketch.edit", json!({"sketch":"Sketch1"})),
        ("sketch.drag", json!({"entity":"l1.start","from":[5,5],"to":[4,5],"live":true})),
    ] {
        s.execute(id, &p).unwrap();
    }
    assert!(s.model.is_stale());
    let model = s.model.state();
    s.evaluations = 0;
    let checkpoint = s.execute("edit.checkpoint", &json!({})).unwrap()["checkpoint"].clone();
    save(&mut s, json!({"exposure_ev":1}));
    s.execute("render.settings.get", &json!({"name":"Studio"})).unwrap();
    s.execute("edit.undo", &json!({})).unwrap();
    s.execute("edit.redo", &json!({})).unwrap();
    s.execute("edit.restore_checkpoint", &json!({"checkpoint":checkpoint})).unwrap();
    assert!(s.doc.render.is_empty());
    assert!(s.model.is_stale());
    assert!(Arc::ptr_eq(&model, &s.model.state()));
    assert_eq!(s.evaluations, 0);
    s.execute("sketch.finish", &json!({})).unwrap();
    assert!(!s.model.is_stale());
    assert!(s.evaluations > 0);
}
