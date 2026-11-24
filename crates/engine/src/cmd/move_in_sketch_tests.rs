//! Move enters a body edit by committing the current sketch first.
use super::*;

fn cube(s: &mut Session) {
    s.execute("solid.box", &json!({"length": 20, "width": 20, "height": 20})).unwrap();
}

fn spec() -> &'static crate::cmd::CommandSpec {
    crate::find_command("solid.move").unwrap()
}

#[test]
fn move_remains_available_as_an_edit_transition_while_sketching() {
    let mut s = Session::default();
    cube(&mut s);
    s.execute("sketch.create", &json!({"plane":"XY"})).unwrap();
    assert!(spec().info(&s).enabled);
    assert!(spec().starts_edit(&json!({})));
}

#[test]
fn running_move_finishes_the_sketch_then_moves_the_body() {
    let mut s = Session::default();
    cube(&mut s);
    let sketch = s.execute("sketch.create", &json!({"plane":"XY"})).unwrap()["sketch"].as_u64().unwrap();
    s.execute("sketch.line", &json!({"points":[[0,0],[5,0]]})).unwrap();
    let reply = s.execute("solid.move", &json!({"bodies":["Body1"],"translate":[30,0,0]})).unwrap();
    assert_eq!(reply["finished_sketch"], s.doc.feature(sketch).unwrap().name);
    assert!(s.active_sketch.is_none());
    let bounds = s.model.state().body("Body1").unwrap().mesh().bounds();
    assert!(bounds.min.dist(Vec3::new(30.0, 0.0, 0.0)) < 1e-6, "{bounds:?}");
}

#[test]
fn move_works_again_once_the_sketch_is_finished() {
    let mut s = Session::default();
    cube(&mut s);
    s.execute("sketch.create", &json!({"plane": "XY"})).unwrap();
    s.execute("sketch.finish", &json!({})).unwrap();
    assert!(spec().info(&s).enabled, "Move comes back once the sketch is finished");

    s.execute("solid.move", &json!({"bodies": ["Body1"], "translate": [30, 0, 0]})).unwrap();
    let bounds = s.model.state().body("Body1").unwrap().mesh().bounds();
    assert!(bounds.min.dist(Vec3::new(30.0, 0.0, 0.0)) < 1e-6, "{bounds:?}");
}
