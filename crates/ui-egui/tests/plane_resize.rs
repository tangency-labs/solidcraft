use serde_json::json;
use solidcraft_engine::Sel;
use solidcraft_ui_egui::scenario::Harness;

fn lid() -> (Harness, u64) {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":40,"width":30,"height":5})).unwrap();
    h.app.run("construct.plane.offset", json!({"base":"XY","offset":10,"name":"Lid"})).unwrap();
    let id = h.app.session.doc.find_feature("Lid").unwrap().id;
    h.call("ui.view", json!({"view":"top"}));
    h.call("ui.view", json!({"view":"fit"}));
    h.app.session.selection = vec![Sel::Plane { name: "Lid".into() }];
    h.frames(3);
    (h, id)
}

/// A selected construction plane is resized by dragging a corner or an edge: the plane keeps
/// the size, as one undo step, and is picked where it now reaches.
#[test]
fn dragging_a_planes_corner_resizes_it() {
    let (mut h, id) = lid();
    let [x0, y0] = h.at(&json!({"handle":"plane_size_1_1"})).expect("a corner handle on the selected plane");
    let [x1, y1] = h.at(&json!({"world":[60,50,10]})).unwrap();
    let undo = h.app.session.undo.len();
    h.call("ui.drag", json!({"x0":x0,"y0":y0,"x1":x1,"y1":y1,"steps":8}));
    h.frames(12);
    let e = *h.app.session.doc.plane_extents.get(&id).expect("the plane keeps its size");
    assert!((e[2] - 60.0).abs() < 0.5 && (e[3] - 50.0).abs() < 0.5, "{e:?}");
    assert!(e[0] < 0.0 && e[1] < 0.0, "the opposite corner stays: {e:?}");
    assert_eq!(h.app.session.undo.len(), undo + 1);
    assert_eq!(h.app.session.selection, vec![Sel::Plane { name: "Lid".into() }], "the plane stays selected");
    // An edge moves one side only.
    let [ex, ey] = h.at(&json!({"handle":"plane_size_-1_0"})).unwrap();
    let [tx, _] = h.at(&json!({"world":[5,0,10]})).unwrap();
    h.call("ui.drag", json!({"x0":ex,"y0":ey,"x1":tx,"y1":ey,"steps":8}));
    h.frames(12);
    let f = *h.app.session.doc.plane_extents.get(&id).unwrap();
    assert!((f[0] - 5.0).abs() < 0.5 && f[1] == e[1] && f[2] == e[2] && f[3] == e[3], "{e:?} -> {f:?}");
    h.app.run("edit.undo", json!({})).unwrap();
    assert_eq!(h.app.session.doc.plane_extents.get(&id), Some(&e));
}

/// Handles only show on a selected construction plane, alone.
#[test]
fn handles_need_the_plane_selected() {
    let (mut h, _) = lid();
    assert!(h.at(&json!({"handle":"plane_size_1_1"})).is_ok());
    h.app.session.selection.clear();
    h.frames(2);
    assert!(h.at(&json!({"handle":"plane_size_1_1"})).is_err());
}
