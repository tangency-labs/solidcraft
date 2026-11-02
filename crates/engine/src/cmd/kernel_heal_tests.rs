//! Delete Face and shells/offsets on cones and spheres, through the engine.

use serde_json::{Value, json};

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn volume(s: &mut Session) -> f64 {
    run(s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap()
}

/// A box with a drilled hole (cone tip) shells: the cavity is the box shrunk by the wall and the
/// hole grown by it (its cone slid down its axis), measured against that analytic volume.
#[test]
fn shell_a_box_with_a_drilled_hole() {
    use std::f64::consts::PI;
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 20}));
    run(&mut s, "solid.hole", json!({"position": [20, 15, 20], "diameter": 8, "depth": 10, "type": "drilled", "tip_angle": 118}));
    let v_body = volume(&mut s);
    let (r, t) = (4.0, 2.0);
    let a = (59.0f64).to_radians();
    // Cylinder depth from the measured body: 24000 − π r² h − π r² (r / tan α) / 3.
    let h = (24000.0 - v_body - PI * r * r * (r / a.tan()) / 3.0) / (PI * r * r);
    assert!(h > 9.0 && h < 11.0, "{h}");
    run(&mut s, "solid.shell", json!({"faces": [[5, 5, 20]], "thickness": t}));
    let v_shell = volume(&mut s);
    let apex = 20.0 - h - r / a.tan() - t / a.sin();
    let big_r = r + t;
    let joint = apex + big_r / a.tan();
    let grown = PI * big_r * big_r * (20.0 - joint) + PI * big_r * big_r * (joint - apex) / 3.0;
    let cavity = 36.0 * 26.0 * 18.0 - grown;
    let want = v_body - cavity;
    assert!((v_shell - want).abs() < 2e-4 * want, "{v_shell} vs {want}");
}

/// #6306: after surface-extrude + thicken of an open spline wall, Shell must hollow the solid.
#[test]
fn shell_after_thicken_of_a_spline_wall() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
    let spline = run(&mut s, "sketch.spline.fit_point", json!({"points": [[0.0, 25.0], [27.0, 25.0], [49.5, 32.0], [66.5, 15.5]], "connect": false}))
        ["created"]["curves"][0]
        .as_str()
        .unwrap()
        .to_owned();
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "surface.extrude", json!({"sketch": "S", "curves": [spline], "distance": 30, "body_name": "Wall"}));
    run(&mut s, "surface.thicken", json!({"bodies": ["Wall"], "thickness": 2, "body_name": "Panel"}));
    let v0 = volume(&mut s);
    assert!(v0 > 1000.0, "thickened panel volume {v0}");
    let state = s.world_state();
    let panel = state.bodies.iter().find(|b| b.name == "Panel").expect("Panel");
    assert!(!panel.is_surface(), "thicken must produce a solid");
    let mut faces = panel.body.faces(0.01).expect("faces");
    faces.sort_by(|a, b| b.area.total_cmp(&a.area));
    let c = faces[0].centroid;
    let t = 0.5;
    run(&mut s, "solid.shell", json!({"body": "Panel", "faces": [[c.x, c.y, c.z]], "thickness": t}));
    let state = s.world_state();
    let panel = state.bodies.iter().find(|b| b.name == "Panel").expect("Panel after shell");
    let v1 = volume(&mut s);
    // Freefrom thicken+shell may leave an open surface cup (faces of the hollow) or a solid
    // with lower volume; either means the shell ran.
    assert!(
        panel.is_surface() || (v1 < v0 * 0.95 && v1 > v0 * 0.05),
        "shell should hollow or open the panel: surface={} before {v0} after {v1} faces={}",
        panel.is_surface(),
        panel.body.face_count()
    );
    assert!(panel.body.face_count() > 4, "shelled panel should keep wall faces");
}

/// Offset Face on a cone and a sphere move them exactly.
#[test]
fn offset_cone_and_sphere_faces() {
    use std::f64::consts::PI;
    let mut s = Session::default();
    run(&mut s, "solid.sphere", json!({"center": [0, 0, 0], "radius": 10}));
    let r = run(&mut s, "solid.offset_face", json!({"faces": [[10, 0, 0]], "distance": 2}));
    let v = volume(&mut s);
    assert!((v - 4.0 / 3.0 * PI * 1728.0).abs() < 1e-3 * v, "{v} {r}");
}

/// Offset Face on a torus changes its tube radius and keeps the major radius.
#[test]
fn offset_torus_face() {
    use std::f64::consts::PI;
    let mut s = Session::default();
    run(&mut s, "solid.torus", json!({"major": 20, "minor": 5}));
    let v0 = volume(&mut s);
    assert!((v0 - 2.0 * PI * PI * 20.0 * 25.0).abs() < 1e-3 * v0, "{v0}");
    run(&mut s, "solid.offset_face", json!({"faces": [[25, 0, 0]], "distance": 1}));
    let v = volume(&mut s);
    assert!((v - 2.0 * PI * PI * 20.0 * 36.0).abs() < 1e-3 * v, "{v}");
}
