//! Quick measurement in the view's corner (#7000): what the selection measures, computed off the
//! window thread, shown above History, copied on a click.

use serde_json::json;
use solidcraft_engine::Sel;
use solidcraft_engine::geom::Vec3;

use crate::scenario::Harness;

/// A design shown, with quick measurement turned on.
fn harness() -> Harness {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.preferences.quick_measure = true;
    h
}

/// A 10 × 20 × 30 box at the origin.
fn block() -> Harness {
    let mut h = harness();
    h.app.run("solid.box", json!({"length": 10, "width": 20, "height": 30})).unwrap();
    h.frames(2);
    h
}

fn edge(h: &Harness, p: Vec3) -> Sel {
    let st = crate::open_meshing::session_world(&h.app.session);
    let m = st.body("Body1").unwrap().mesh();
    let index = m.edges.iter().position(|e| e.windows(2).any(|w| p.dist_to_segment(w[0], w[1]) < 1e-9)).expect("an edge there");
    Sel::Edge { body: "Body1".into(), index, point: p }
}

/// The face with a triangle wholly at height `z`.
fn face_at_height(h: &Harness, z: f64) -> Sel {
    let st = crate::open_meshing::session_world(&h.app.session);
    let m = st.body("Body1").unwrap().mesh();
    let (t, f) = m.triangles.iter().zip(&m.tri_face).find(|(t, _)| m.tri(t).is_some_and(|ps| ps.iter().all(|p| (p.z - z).abs() < 1e-9))).unwrap();
    let ps = m.tri(t).unwrap();
    Sel::Face { body: "Body1".into(), index: *f as usize, point: (ps[0] + ps[1] + ps[2]) * (1.0 / 3.0) }
}

/// The readout once its measurement is in (empty if none comes).
fn readout(h: &mut Harness) -> Vec<(String, String)> {
    for _ in 0..300 {
        // As each frame does before drawing it: a changed selection is measured afresh.
        crate::quick_measure::poll(&mut h.app, &h.ctx);
        let rows = h.app.quick_measure.lines(&h.app);
        if !rows.is_empty() {
            return rows;
        }
        h.frames(1);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    Vec::new()
}

fn row(label: &str, value: String) -> (String, String) {
    (label.to_string(), value)
}

#[test]
fn an_edge_shows_its_length() {
    let mut h = block();
    h.app.session.selection = vec![edge(&h, Vec3::new(5.0, 0.0, 0.0))];
    let length = crate::prefs::show_mm(&h.app, 10.0, 1);
    assert_eq!(readout(&mut h), vec![row("Length", length)]);
}

#[test]
fn a_round_edge_shows_its_diameter_radius_and_circumference() {
    let mut h = harness();
    h.app.run("solid.cylinder", json!({"radius": 5, "height": 10})).unwrap();
    h.frames(2);
    // The top circle, by a point between two of its polyline's points (its first is on the seam).
    let on_rim = {
        let st = crate::open_meshing::session_world(&h.app.session);
        let m = st.body("Body1").unwrap().mesh();
        let rim = m.edges.iter().find(|e| e.len() > 2 && e.iter().all(|p| (p.z - 10.0).abs() < 1e-9)).expect("the top circle");
        (rim[1] + rim[2]) * 0.5
    };
    h.app.session.selection = vec![edge(&h, on_rim)];
    let rows = readout(&mut h);
    assert_eq!(rows.first(), Some(&row("Ø", crate::prefs::show_mm(&h.app, 10.0, 1))), "{rows:?}");
    assert_eq!(rows.get(1), Some(&row("R", crate::prefs::show_mm(&h.app, 5.0, 1))), "{rows:?}");
    assert!(rows.iter().any(|(l, _)| l == "Circumference"), "{rows:?}");
}

#[test]
fn a_face_shows_its_area() {
    let mut h = block();
    h.app.session.selection = vec![face_at_height(&h, 30.0)];
    assert_eq!(readout(&mut h), vec![row("Area", crate::prefs::show_mm(&h.app, 200.0, 2))]);
}

#[test]
fn a_sketch_line_shows_its_length() {
    let mut h = harness();
    h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
    h.app.run("sketch.line", json!({"points": [[0, 0], [30, 40]]})).unwrap();
    h.app.session.selection = vec![Sel::SketchCurve { id: "l1".into(), sketch: None }];
    assert_eq!(readout(&mut h), vec![row("Length", crate::prefs::show_mm(&h.app, 50.0, 1))]);
}

/// A sketch that is only shown, not open: a click picks its curves, points and regions, and they
/// measure as they do in the sketch.
#[test]
fn a_shown_sketchs_line_point_and_region_measure_too() {
    let mut h = harness();
    let sketch = h.app.run("sketch.create", json!({"plane": "XY"})).unwrap()["sketch"].as_u64().unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [30, 40]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    assert_eq!(h.app.session.active_sketch, None);
    let sk = h.app.session.doc.sketch(sketch).unwrap();
    let side = sk.curves.first().unwrap().id.clone();
    let corner = sk.points.first().unwrap().id.clone();
    h.app.session.selection = vec![Sel::ExternalSketchCurve { sketch, id: side }];
    let rows = readout(&mut h);
    assert!(rows.iter().any(|(l, v)| l == "Length" && [30.0, 40.0].iter().any(|n| *v == crate::prefs::show_mm(&h.app, *n, 1))), "{rows:?}");
    h.app.session.selection = vec![Sel::ExternalSketchPoint { sketch, id: corner }];
    assert!(readout(&mut h).iter().any(|(l, _)| l == "At"), "a point's position");
    h.app.session.selection = vec![Sel::Profile { sketch, index: 0 }];
    assert_eq!(readout(&mut h), vec![row("Area", crate::prefs::show_mm(&h.app, 1200.0, 2))]);
}

#[test]
fn two_points_show_their_distance() {
    let mut h = block();
    h.app.session.selection = [[0.0, 0.0, 0.0], [10.0, 20.0, 30.0]].map(|p| Sel::Vertex { body: "Body1".into(), point: p.into() }).to_vec();
    let rows = readout(&mut h);
    assert!(rows.contains(&row("Distance", crate::prefs::show_mm(&h.app, 1400f64.sqrt(), 1))), "{rows:?}");
}

#[test]
fn several_edges_show_their_count_and_total_length() {
    let mut h = block();
    h.app.session.selection = [[5.0, 0.0, 0.0], [5.0, 20.0, 0.0], [5.0, 0.0, 30.0]].map(|[x, y, z]| edge(&h, Vec3::new(x, y, z))).to_vec();
    assert_eq!(readout(&mut h), vec![row("3 edges", crate::prefs::show_mm(&h.app, 30.0, 1))]);
}

/// Off by default: nothing shows until Preferences › Interface turns it on.
#[test]
fn quick_measurement_is_off_until_turned_on() {
    assert!(!crate::prefs::Prefs::default().quick_measure);
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.run("solid.box", json!({"length": 10, "width": 20, "height": 30})).unwrap();
    h.frames(2);
    assert!(!h.app.preferences.quick_measure);
    h.app.session.selection = vec![edge(&h, Vec3::new(5.0, 0.0, 0.0))];
    assert!(readout(&mut h).is_empty(), "off by default");
    h.frames(2);
    assert!(crate::scenario::rect_of("measure:quick").is_none(), "nothing drawn");
    h.call("ui.prefs", json!({"quick_measure": true}));
    assert_eq!(readout(&mut h), vec![row("Length", crate::prefs::show_mm(&h.app, 10.0, 1))]);
}

/// Nothing shows with the preference off, with nothing selected, for something with nothing to
/// measure (a body) or while the Measure tool, which says the same, is open.
#[test]
fn the_readout_stays_away_when_it_has_nothing_to_add() {
    let mut h = block();
    let e = edge(&h, Vec3::new(5.0, 0.0, 0.0));
    h.app.session.selection = vec![e.clone()];
    assert!(!readout(&mut h).is_empty());
    h.call("ui.prefs", json!({"quick_measure": false}));
    h.frames(2);
    assert!(h.app.quick_measure.lines(&h.app).is_empty(), "preference off");
    h.call("ui.prefs", json!({"quick_measure": true}));
    h.app.session.selection.clear();
    h.frames(2);
    assert!(h.app.quick_measure.lines(&h.app).is_empty(), "nothing selected");
    h.app.session.selection = vec![Sel::Body { name: "Body1".into() }];
    h.frames(2);
    assert!(readout(&mut h).is_empty(), "a body");
    h.app.session.selection = vec![e];
    h.app.start("inspect.measure");
    h.frames(2);
    assert!(h.app.quick_measure.lines(&h.app).is_empty(), "the Measure tool is open");
}

/// The readout sits in the view's bottom right corner above History; a click on it copies it.
#[test]
fn the_readout_sits_above_history_and_a_click_copies_it() {
    let mut h = block();
    h.app.session.selection = vec![edge(&h, Vec3::new(5.0, 0.0, 0.0))];
    assert!(!readout(&mut h).is_empty());
    h.frames(2);
    let r = crate::scenario::rect_of("measure:quick").expect("the readout is drawn");
    let history = h.app.shell.history.expect("History is shown");
    assert!(r.bottom() <= history.top(), "{r:?} above History {history:?}");
    assert!(r.right() > 1600.0 * 0.6, "{r:?} on the right");
    let at = r.center();
    let button = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
    h.frame_with(vec![egui::Event::PointerMoved(at)]);
    h.frame_with(vec![button(true)]);
    h.frame_with(vec![button(false)]);
    let copied = h.output.commands.iter().find_map(|c| if let egui::OutputCommand::CopyText(t) = c { Some(t.clone()) } else { None });
    assert_eq!(copied, Some(format!("Length {}", crate::prefs::show_mm(&h.app, 10.0, 1))));
}

/// The measurement is computed off the window thread and lands a frame or so after the selection
/// changes. The thread may already be done when the first poll looks, so only the arrival is
/// asserted, never that it is still coming.
#[test]
fn the_measurement_computes_off_the_window_thread() {
    let mut h = block();
    h.app.session.selection = vec![edge(&h, Vec3::new(5.0, 0.0, 0.0))];
    crate::quick_measure::poll(&mut h.app, &h.ctx);
    assert!(!readout(&mut h).is_empty(), "then in");
}
