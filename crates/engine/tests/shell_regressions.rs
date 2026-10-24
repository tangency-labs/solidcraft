//! Shell regressions found by the robustness fuzzer (tests/robustness/goldens.json).
//!
//! The box-on-box designs below have a saddle corner: where the top box's left face meets the
//! lower box's back face, four planar faces meet with convex and concave edges in turn (the
//! top box overhangs the lower box's back face). A shell splits that corner into two corners
//! joined by a new edge.
//!
//! Lower box `[0, 25] × [0, 17.5] × [0, 35]`, top box `[7.5, 22.5] × [0, 17.5 + o] × [35, 42.5]`.
//! With the bottom face open, a shell of thickness `t` leaves this cavity (for `o < 17.5 − 2t`):
//!
//! - in the lower box: `[t, 25 − t] × [t, 17.5 − t] × [0, 35 − t]`
//! - in the top box, over the lower box: `[7.5 + t, 22.5 − t] × [t, 17.5 − t] × [35 − t, 42.5 − t]`
//! - in the overhang: `[7.5 + t, 22.5 − t] × [17.5 − t, 17.5 + o − t] × [35 + t, 42.5 − t]`
//!
//! The shell's volume is the solid's volume less those three boxes.
use serde_json::json;
use solidcraft_engine::Session;

fn run(s: &mut Session, id: &str, p: serde_json::Value) {
    if let Err(e) = s.execute(id, &p) {
        panic!("{id} {p}: {e}");
    }
}

/// The box on a box, overhanging the lower box's back face by `o`.
fn box_on_box(o: f64) -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"height": 35.0, "length": 25.0, "width": 17.5}));
    run(&mut s, "solid.box", json!({"corner": [7.5, 0.0, 35.0], "height": 7.5, "length": 15.0, "operation": "join", "width": 17.5 + o}));
    s
}

/// The shell's volume with the bottom face open (see the module comment).
fn open_bottom_shell_volume(o: f64, t: f64) -> f64 {
    let solid = 25.0 * 17.5 * 35.0 + 15.0 * (17.5 + o) * 7.5;
    let lower = (25.0 - 2.0 * t) * (17.5 - 2.0 * t) * (35.0 - t);
    let above = (15.0 - 2.0 * t) * (17.5 - 2.0 * t) * 7.5;
    let overhang = (15.0 - 2.0 * t) * o * (7.5 - 2.0 * t);
    solid - lower - above - overhang
}

/// Seed 83 (minimised): the overhang is 2.5 mm, and the shell opens the top box's left face.
/// The shell refused the saddle corner (`a corner whose faces don't meet the same way`).
#[test]
fn shell_box_on_box_overhanging_saddle_corner() {
    let mut s = box_on_box(2.5);
    run(&mut s, "solid.shell", json!({"faces": [[7.5, 17.5, 38.75]], "thickness": 1.5}));
    sound(&s);
}

/// The same saddle corner with the bottom open: every face round the corner moves, and the
/// volume is exact.
#[test]
fn shell_saddle_corner_volume_is_exact() {
    for t in [1.0, 1.5] {
        let mut s = box_on_box(2.5);
        run(&mut s, "solid.shell", json!({"faces": [[12.5, 8.75, 0.0]], "thickness": t}));
        sound(&s);
        let (got, want) = (volume(&s), open_bottom_shell_volume(2.5, t));
        assert!((got - want).abs() < 1e-6 * want, "thickness {t}: shell volume {got}, want {want}");
    }
}

/// An overhang (1 mm) thinner than the wall (1.5 mm): the overhang's cavity is only 1 mm deep.
/// A corner landing outside the solid, or a wall left thin, would change the volume.
#[test]
fn shell_saddle_corner_with_an_overhang_thinner_than_the_wall() {
    let mut s = box_on_box(1.0);
    run(&mut s, "solid.shell", json!({"faces": [[12.5, 8.75, 0.0]], "thickness": 1.5}));
    sound(&s);
    let (got, want) = (volume(&s), open_bottom_shell_volume(1.0, 1.5));
    assert!((got - want).abs() < 1e-6 * want, "shell volume {got}, want {want}");
}

fn volume(s: &Session) -> f64 {
    s.model.state().bodies.iter().map(|b| solidcraft_kernel::measure(&b.body).map(|m| m.volume).unwrap_or(f64::NAN)).sum()
}

fn sound(s: &Session) {
    let errors: Vec<_> = s.model.results.iter().filter_map(|r| r.error.clone()).collect();
    assert!(errors.is_empty(), "{errors:?}");
    let st = s.model.state();
    assert_eq!(st.bodies.len(), 1, "one body");
    for b in &st.bodies {
        assert_eq!(b.body.shell_count(), 1, "one shell");
        assert!(b.body.validity().is_empty(), "{:?}", b.body.validity());
    }
}

/// Seed 6 (minimised, #1494): a cylinder joined on a box's side leaves a saddle on the
/// cylinder seam. The shell used to refuse that corner.
#[test]
fn shell_box_with_side_cylinder_seam_saddle() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"height": 30.0, "length": 47.5, "width": 45.0}));
    run(&mut s, "solid.cylinder", json!({"axis": [1.0, 0.0, 0.0], "base": [47.5, 0.0, 10.0], "height": 30.0, "operation": "join", "radius": 7.5}));
    run(&mut s, "solid.shell", json!({"faces": [[23.75, 22.5, 0.0]], "thickness": 1.0}));
    sound(&s);
}

/// #566: a rectangular section swept along a path with a bend. Shelling used to refuse the
/// concave bend corner where a plane meets the inner cylinder.
#[test]
fn shell_swept_bar_with_a_bend() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XZ", "name": "Path"}));
    run(&mut s, "sketch.line", json!({"points": [[0.0, 0.0], [0.0, 30.0]], "ids": ["l1"]}));
    run(
        &mut s,
        "sketch.arc.three_point",
        json!({"start": "l1.end", "through": [8.787, 51.213], "end": [30.0, 60.0], "id": "a1"}),
    );
    run(&mut s, "sketch.line", json!({"points": ["a1.end", [60.0, 60.0]], "ids": ["l2"]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Section"}));
    run(&mut s, "sketch.rectangle.center", json!({"center": [0.0, 0.0], "corner": [4.0, 2.5]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.sweep", json!({"sketch": "Section", "path_sketch": "Path", "path": ["l1", "a1", "l2"]}));
    run(&mut s, "solid.shell", json!({"faces": [[0.0, 0.0, 0.0]], "thickness": 0.5}));
    sound(&s);
    let vol = volume(&s);
    assert!(vol.is_finite() && vol > 0.0 && vol < 2000.0, "shell volume {vol}");
}

/// #4010: a plate with a through hole, shelled by opening only the hole wall. Volume is the
/// plate less an inset box that keeps the same bore (analytic 8044.80 mm³).
#[test]
fn shell_plate_opening_the_hole_wall() {
    let mut s = Session::default();
    run(
        &mut s,
        "sketch.create",
        json!({"plane": {"origin": [0.0, 0.0, 0.0], "x_dir": [1.0, 0.0, 0.0], "y_dir": [0.0, 1.0, 0.0]}}),
    );
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0.0, 0.0], "p1": [40.0, 30.0]}));
    run(&mut s, "sketch.circle.center", json!({"center": [20.0, 15.0], "radius": 5.0}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 12.5, "operation": "new", "profiles": [{"point": [2.0, 2.0]}]}));
    run(&mut s, "solid.shell", json!({"faces": [[25.0, 15.0, 6.25]], "thickness": 2.5}));
    sound(&s);
    let got = volume(&s);
    let want = 8044.80;
    assert!((got - want).abs() < 1.0, "shell volume {got}, want ~{want}");
}

/// Seed 78 (minimised, #142): a half-revolve cut by a box. A shell thicker than the rim used to
/// look like a missed corner; it is refused as a wall cross.
#[test]
fn shell_half_revolve_cut_by_box_names_wall_cross() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XZ"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [7.5, 0.0], "p1": [25.0, 5.0]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.revolve", json!({"angle": "180 deg", "axis": "Z"}));
    run(
        &mut s,
        "solid.box",
        json!({"corner": [-25.0, 0.0, -2.5], "height": 22.5, "length": 17.5, "operation": "cut", "width": 22.5}),
    );
    let err = s
        .execute("solid.shell", &json!({"faces": [[16.25, 0.0, -2.5]], "thickness": 2.0}))
        .expect_err("thick shell");
    let msg = err.to_string();
    assert!(msg.contains("faces would cross"), "{msg}");
    assert!(!msg.contains("don't meet the same way"), "{msg}");
}

/// Seed 38 (minimised, #142): a cylindrical notch on a box edge. A shell thicker than the notch
/// used to look like a missed corner; it is refused as a wall cross.
#[test]
fn shell_box_with_edge_notch_names_wall_cross() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"height": 27.5, "length": 47.5, "width": 60.0}));
    run(
        &mut s,
        "solid.cylinder",
        json!({"axis": [0.0, 0.0, -1.0], "base": [47.5, 0.0, 25.0], "height": 22.5, "operation": "cut", "radius": 1.25}),
    );
    let err = s
        .execute("solid.shell", &json!({"faces": [[47.5, 30.509393063583815, 13.75]], "thickness": 2.0}))
        .expect_err("thick shell");
    let msg = err.to_string();
    assert!(msg.contains("faces would cross"), "{msg}");
    assert!(!msg.contains("don't meet the same way"), "{msg}");
}

/// Seed 69 (minimised, #142): a 90° revolve with a joined box, shelled twice. The second shell
/// used to refuse a corner on the revolved wall.
#[test]
fn shell_revolved_section_with_joined_box_twice() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XZ"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [2.5, 0.0], "p1": [22.5, 30.0]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.revolve", json!({"angle": "90 deg", "axis": "Z"}));
    run(
        &mut s,
        "solid.box",
        json!({"corner": [22.5, 17.5, -7.5], "height": 5.0, "length": 22.5, "operation": "join", "width": 7.5}),
    );
    run(&mut s, "solid.shell", json!({"faces": [[33.75, 17.5, -5.0]], "thickness": 1.5}));
    run(
        &mut s,
        "solid.shell",
        json!({"faces": [[1.5, 12.327229847555518, -15.0]], "thickness": 1.5}),
    );
    sound(&s);
}
