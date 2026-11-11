//! The two worked examples in `skills/solidcraft-sheetmetal/reference.md` build to the volumes the
//! skill documents: a 2 mm, K 0.5, R 1 mm base 200 × 100 mm with two stretches on the back edge
//! (90°/20 mm over 150 mm, then 45°/15 mm over 50 mm) and the three-stretch variant. A change that
//! moves the bend geometry shows up here as a different volume.
use serde_json::{Value, json};
use solidcraft_engine::Session;

fn run(s: &mut Session, id: &str, params: Value) -> Value {
    s.execute(id, &params).unwrap_or_else(|e| panic!("{id} {params}: {e}"))
}

/// The folded volume the model reports.
fn volume(s: &mut Session) -> f64 {
    run(s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap_or(f64::NAN)
}

fn base(s: &mut Session) {
    run(s, "sheet.manage_rules", json!({"name":"AL2","thickness":"2 mm","k_factor":0.5,"bend_radius":"1 mm","active":true}));
    run(s, "sketch.create", json!({"plane":"XY","name":"Base"}));
    run(s, "sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[200,100]}));
    run(s, "sketch.finish", json!({}));
    run(s, "sheet.flange", json!({"sketch":"Base"}));
}

#[test]
fn the_documented_two_stretch_example_builds_to_its_volume() {
    let mut s = Session::default();
    base(&mut s);
    run(
        &mut s,
        "sheet.flange",
        json!({"type":"edge","edges":[{"point":[100,0,2],"segments":[
            {"from":0,"to":0.75,"angle":"90 deg","height":"20 mm"},
            {"from":0.75,"to":1,"angle":"45 deg","height":"15 mm"}]}],
            "height":"20 mm","angle":"90 deg","position":"inside"}),
    );
    let errors: Vec<String> = s.model.results.iter().filter_map(|r| r.error.clone()).collect();
    assert!(errors.is_empty(), "{errors:?}");
    let v = volume(&mut s);
    assert!((v - 46551.02929133257).abs() / 46551.02929133257 < 1e-9, "documented volume 46551.02929133257 mm³, built {v}");
}

#[test]
fn the_documented_three_stretch_example_builds_to_its_volume() {
    let mut s = Session::default();
    base(&mut s);
    run(
        &mut s,
        "sheet.flange",
        json!({"type":"edge","edges":[{"point":[100,0,2],"segments":[
            {"from":0,"to":0.5,"angle":"90 deg","height":"20 mm"},
            {"from":0.5,"to":0.75,"angle":"90 deg","height":"15 mm"},
            {"from":0.75,"to":1,"angle":"45 deg","height":"15 mm"}]}],
            "height":"20 mm","angle":"90 deg","position":"inside"}),
    );
    let errors: Vec<String> = s.model.results.iter().filter_map(|r| r.error.clone()).collect();
    assert!(errors.is_empty(), "{errors:?}");
    let v = volume(&mut s);
    assert!((v - 46051.02928860431).abs() / 46051.02928860431 < 1e-9, "documented volume 46051.02928860431 mm³, built {v}");
}
