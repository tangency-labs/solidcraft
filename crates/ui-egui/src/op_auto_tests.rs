//! Feature windows choose Join, New Body or Cut from what their material does (#6901).

use std::time::{Duration, Instant};

use serde_json::json;
use solidcore_record::Vec3;

use super::{CUT, JOIN, NEW};
use crate::dialogs::Kind;
use crate::scenario::Harness;

/// A 10 mm cube at the origin, named `Base`.
fn cube() -> Harness {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Base"})).unwrap();
    h
}

fn rect_sketch(h: &mut Harness, p0: [f64; 2], p1: [f64; 2]) {
    h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0": p0, "p1": p1})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.app.run("select.clear", json!({})).unwrap();
}

/// Frames until the window's check is in.
fn checked(h: &mut Harness) {
    let began = Instant::now();
    while (h.app.op_check.busy() || h.app.dialog.as_ref().is_some_and(|d| d.op_auto.pending)) && began.elapsed() < Duration::from_secs(60) {
        h.frames(1);
        std::thread::sleep(Duration::from_millis(1));
    }
    h.frames(2);
}

fn op(h: &mut Harness) -> usize {
    match h.app.dialog.as_mut().map(|d| &mut d.kind) {
        Some(
            Kind::Extrude { operation, .. } | Kind::Revolve { operation, .. } | Kind::Primitive { operation, .. } | Kind::Loft { operation, .. },
        ) => *operation,
        other => panic!("a feature window: {other:?}"),
    }
}

/// Pick an operation in the window's list (the shown one too): what [`super::op_row`] notes.
fn pick(h: &mut Harness, to: usize) {
    if let Some(d) = h.app.dialog.as_mut() {
        match &mut d.kind {
            Kind::Extrude { operation, .. } | Kind::Revolve { operation, .. } | Kind::Primitive { operation, .. } | Kind::Loft { operation, .. } => {
                *operation = to
            }
            other => panic!("a feature window: {other:?}"),
        }
    }
    h.ctx.data_mut(|m| m.insert_temp(super::picked_id(), true));
}

fn bodies(h: &Harness) -> Vec<String> {
    h.app.session.model.state().bodies.iter().map(|b| b.name.clone()).collect()
}

fn ok(h: &mut Harness) {
    h.step(&json!({"key": "Enter"})).unwrap();
    checked(h);
    let began = Instant::now();
    while h.app.dialog.is_some() && began.elapsed() < Duration::from_secs(60) {
        h.frames(1);
    }
    assert!(h.app.dialog.is_none(), "OK closes the window");
}

#[test]
fn an_extrude_apart_is_a_new_body_and_one_touching_joins() {
    for (p0, p1, want) in [([30.0, 0.0], [40.0, 10.0], NEW), ([10.0, 0.0], [20.0, 10.0], JOIN), ([10.0, 5.0], [20.0, 15.0], JOIN)] {
        let mut h = cube();
        rect_sketch(&mut h, p0, p1);
        h.step(&json!({"start": "solid.extrude"})).unwrap();
        checked(&mut h);
        assert_eq!(op(&mut h), want, "{p0:?}–{p1:?}");
        // A Join the check evaluated is OK's: it isn't computed again.
        let cmds = h.app.dialog.as_ref().and_then(|d| crate::dialogs::apply_commands(&h.app, d).ok()).unwrap_or_default();
        assert_eq!(h.app.op_check.joined_for(&cmds, h.app.session.revision).is_some(), want == JOIN, "{p0:?}–{p1:?}");
        ok(&mut h);
        assert_eq!(bodies(&h).len(), if want == NEW { 2 } else { 1 }, "{p0:?}–{p1:?}");
    }
}

#[test]
fn with_the_preference_off_the_window_keeps_its_operation() {
    assert!(serde_json::from_value::<crate::prefs::Prefs>(json!({})).unwrap().auto_operation, "on for saved settings without it");
    let mut h = cube();
    h.app.preferences.auto_operation = false;
    rect_sketch(&mut h, [30.0, 0.0], [40.0, 10.0]);
    h.step(&json!({"start": "solid.extrude"})).unwrap();
    h.frames(3);
    assert!(!h.app.op_check.busy(), "no check runs");
    assert!(h.app.dialog.as_ref().is_some_and(|d| !d.op_auto.pending));
    assert_eq!(op(&mut h), JOIN, "Extrude keeps its own Join, as before");
    ok(&mut h);
    assert_eq!(bodies(&h).len(), 1, "the Join apart makes one body in two pieces");
}

#[test]
fn with_new_body_by_default_the_window_opens_and_stays_in_new_body() {
    assert!(!serde_json::from_value::<crate::prefs::Prefs>(json!({})).unwrap().new_body_default, "off for saved settings without it");
    // An extrude touching the cube would join, and a sphere inside it would cut.
    let mut h = cube();
    h.app.preferences.new_body_default = true;
    rect_sketch(&mut h, [10.0, 0.0], [20.0, 10.0]);
    h.step(&json!({"start": "solid.extrude"})).unwrap();
    h.frames(3);
    assert!(!h.app.op_check.busy(), "no check runs");
    assert_eq!(op(&mut h), NEW);
    ok(&mut h);
    assert_eq!(bodies(&h).len(), 2, "the touching extrude is a body of its own");
    let mut h = cube();
    h.app.preferences.new_body_default = true;
    h.step(&json!({"start": "solid.sphere"})).unwrap();
    if let Some(Kind::Primitive { fields, at, .. }) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
        fields.iter_mut().for_each(|(_, v)| *v = "4 mm".into());
        *at = Some((Vec3::new(5.0, 5.0, 5.0), Some(Vec3::Z)));
    }
    h.frames(3);
    assert_eq!(op(&mut h), NEW, "the sphere inside the cube stays a new body");
    // Picked by hand, another operation is the window's.
    pick(&mut h, JOIN);
    h.frames(2);
    assert_eq!(op(&mut h), JOIN);
}

#[test]
fn closing_the_window_stops_its_check() {
    // A larger body, so the check is still running when the window closes.
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length": 40, "width": 40, "height": 10, "corner": [-20, -20, 0], "body_name": "Base"})).unwrap();
    for (x, y) in [(-10, -10), (10, -10), (-10, 10), (10, 10)] {
        h.app.run("solid.cylinder", json!({"radius": 3, "height": 10, "base": [x, y, 0], "operation": "cut"})).unwrap();
    }
    rect_sketch(&mut h, [20.0, -5.0], [30.0, 5.0]);
    h.app.start("solid.extrude");
    h.frames(1);
    let progress = h.app.op_check.progress.clone();
    h.app.dialog = None;
    h.frames(1);
    assert!(!h.app.op_check.busy(), "nothing waits for a window that is gone");
    assert!(h.app.op_check.done.is_none() && h.app.op_check.joined.is_none(), "what it found is forgotten");
    if let Some(progress) = progress {
        assert!(progress.is_cancelled(), "the running check was told to stop");
    }
}

#[test]
fn a_ring_round_a_hole_joins() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length": 40, "width": 40, "height": 10, "corner": [-20, -20, 0], "body_name": "Base"})).unwrap();
    h.app.run("solid.cylinder", json!({"radius": 5, "height": 10, "operation": "cut"})).unwrap();
    h.app.run("sketch.create", json!({"plane": {"origin": [0, 0, 10], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}})).unwrap();
    h.app.run("sketch.circle.center", json!({"center": [0, 0], "radius": 5})).unwrap();
    h.app.run("sketch.circle.center", json!({"center": [0, 0], "radius": 10})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.app.run("select.clear", json!({})).unwrap();
    h.step(&json!({"start": "solid.extrude"})).unwrap();
    checked(&mut h);
    if let Some(d) = h.app.dialog.as_mut() {
        // The ring alone, not the disc inside it.
        d.inputs[0].items.retain(|s| {
            matches!(s, solidcraft_engine::Sel::Profile { index, .. } if *index == 1) || !matches!(s, solidcraft_engine::Sel::Profile { .. })
        });
    }
    checked(&mut h);
    assert_eq!(op(&mut h), JOIN);
}

#[test]
fn primitives_and_revolves_join_cut_or_stand_apart() {
    let at = |h: &mut Harness, point: [f64; 3]| {
        if let Some(Kind::Primitive { at, .. }) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
            *at = Some((Vec3::new(point[0], point[1], point[2]), Some(Vec3::Z)));
        }
    };
    for (point, want) in [([50.0, 0.0, 0.0], NEW), ([5.0, 5.0, 10.0], JOIN), ([5.0, 5.0, 5.0], CUT)] {
        let mut h = cube();
        h.step(&json!({"start": "solid.sphere"})).unwrap();
        if let Some(Kind::Primitive { fields, .. }) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
            fields.iter_mut().for_each(|(_, v)| *v = "4 mm".into());
        }
        at(&mut h, point);
        checked(&mut h);
        assert_eq!(op(&mut h), want, "sphere at {point:?}");
    }
}

#[test]
fn a_hand_picked_operation_stays_even_while_the_check_runs() {
    // Picked before the first check is in: that check doesn't undo it.
    let mut h = cube();
    rect_sketch(&mut h, [30.0, 0.0], [40.0, 10.0]);
    h.step(&json!({"start": "solid.extrude"})).unwrap();
    pick(&mut h, JOIN);
    checked(&mut h);
    assert_eq!(op(&mut h), JOIN, "Join picked by hand stays, apart or not");
    // The same in a window without Extrude's own choice.
    let mut h = cube();
    h.step(&json!({"start": "solid.sphere"})).unwrap();
    h.frames(1);
    pick(&mut h, NEW);
    checked(&mut h);
    assert_eq!(op(&mut h), NEW, "New Body picked by hand on a body stays");
}

#[test]
fn ok_runs_what_the_check_chose() {
    // Enter at once: OK waits for the check, then makes a new body for the block apart.
    let mut h = cube();
    rect_sketch(&mut h, [30.0, 0.0], [40.0, 10.0]);
    h.step(&json!({"start": "solid.extrude"})).unwrap();
    ok(&mut h);
    assert_eq!(bodies(&h).len(), 2, "{:?}", bodies(&h));
}

#[test]
fn hidden_bodies_take_no_part() {
    let mut h = cube();
    h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [40, 0, 0], "body_name": "Other"})).unwrap();
    h.app.run("browser.visibility", json!({"items": ["Base"], "visible": false})).unwrap();
    // A block on the hidden cube: apart from what is shown.
    rect_sketch(&mut h, [10.0, 0.0], [20.0, 10.0]);
    h.step(&json!({"start": "solid.extrude"})).unwrap();
    checked(&mut h);
    assert_eq!(op(&mut h), NEW, "the hidden cube is not joined");
    // Join picked by hand joins only the shown bodies: the hidden cube stays as it is.
    pick(&mut h, JOIN);
    ok(&mut h);
    let st = h.app.session.model.state();
    assert!(st.body("Base").is_some_and(|b| b.body.face_count() == 6), "the hidden cube is untouched");
    // Nothing shown: New Body at once.
    let mut h = cube();
    h.app.run("browser.visibility", json!({"items": ["Base"], "visible": false})).unwrap();
    rect_sketch(&mut h, [10.0, 0.0], [20.0, 10.0]);
    h.step(&json!({"start": "solid.extrude"})).unwrap();
    h.frames(2);
    assert_eq!(op(&mut h), NEW);
}

/// Pipe and Thicken don't choose their operation, yet a Cut from their windows leaves a hidden
/// body alone too: two 20 mm boxes, the second hidden, a path through both at half height.
#[test]
fn a_pipe_or_thicken_cut_from_its_window_leaves_hidden_bodies_alone() {
    let volume = |h: &Harness, name: &str| {
        let st = crate::open_meshing::session_world(&h.app.session);
        st.body(name).and_then(|b| solidcraft_engine::doc::kernel::measure(&b.body).ok()).map(|m| m.volume)
    };
    let mut found = Vec::new();
    for tool in ["solid.pipe", "surface.thicken"] {
        let mut h = Harness::new();
        h.app.run("solid.box", json!({"length": 20, "width": 20, "height": 20, "body_name": "A"})).unwrap();
        h.app.run("solid.box", json!({"length": 20, "width": 20, "height": 20, "corner": [40, 0, 0], "body_name": "B"})).unwrap();
        h.app.run("construct.plane.offset", json!({"base": "XY", "offset": 10, "name": "Mid"})).unwrap();
        h.app.run("sketch.create", json!({"plane": "Mid"})).unwrap();
        h.app.run("sketch.line", json!({"points": [[-10, 10], [70, 10]], "ids": ["path"]})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        let mut sels = vec![solidcraft_engine::Sel::SketchCurve { id: "path".into(), sketch: None }];
        if tool == "surface.thicken" {
            h.app.run("surface.extrude", json!({"curves": ["path"], "distance": 4, "body_name": "Sheet"})).unwrap();
            sels = vec![solidcraft_engine::Sel::Body { name: "Sheet".into() }];
        }
        h.app.run("browser.visibility", json!({"items": ["B"], "visible": false})).unwrap();
        h.app.session.selection = sels;
        h.app.start(tool);
        let Some(mut d) = h.app.dialog.clone() else { panic!("{tool}: no window") };
        match &mut d.kind {
            Kind::Pipe { diameter, operation, .. } => (*diameter, *operation) = ("6 mm".into(), CUT),
            Kind::Surface(crate::dialogs_surface::Surf::Thicken { thickness, operation, .. }) => (*thickness, *operation) = ("2 mm".into(), CUT),
            other => panic!("{tool}: {other:?}"),
        }
        for (cmd, p) in crate::dialogs::apply_commands(&h.app, &d).unwrap() {
            h.app.run(&cmd, p).unwrap();
        }
        // Editing it keeps the shown box as its only target.
        let id = h.app.session.doc.features.last().map(|f| f.id);
        let edit = id.and_then(|id| crate::dialogs::for_feature(&h.app, id, None)).and_then(|d| crate::dialogs::apply_commands(&h.app, &d).ok());
        let kept = edit.as_ref().and_then(|c| c.first()).map(|(_, p)| p["params"]["targets"].clone());
        found.push((tool, volume(&h, "A").unwrap_or_default(), volume(&h, "B").unwrap_or_default(), kept));
    }
    // Pipe: 6 mm across 20 mm takes 180π (565.49 mm3); Thicken: 20 x 4 x 2 takes 160 mm3.
    for (tool, a, b, kept) in &found {
        let cut = if *tool == "solid.pipe" { 180.0 * std::f64::consts::PI } else { 160.0 };
        assert!(
            (a - (8000.0 - cut)).abs() < 1.0 && (b - 8000.0).abs() < 1e-6 && *kept == Some(json!(["A"])),
            "the shown box A is cut, the hidden B untouched, an edit keeps A as the target: {found:?}"
        );
    }
}

/// An Emboss or Deboss with no Target Face joins or cuts every body its profile reaches; from its
/// window it leaves a hidden body alone too: two 20 mm boxes, the second hidden, a 10 mm square
/// over each on the plane of their tops. With every body hidden it is refused. An Emboss with a
/// Target Face keeps that face's body as its one target.
#[test]
fn an_emboss_with_no_target_face_from_its_window_leaves_hidden_bodies_alone() {
    let volume = |h: &Harness, name: &str| {
        let st = crate::open_meshing::session_world(&h.app.session);
        st.body(name).and_then(|b| solidcraft_engine::doc::kernel::measure(&b.body).ok()).map(|m| m.volume)
    };
    let setup = |hide: &[&str]| {
        let mut h = Harness::new();
        h.app.run("solid.box", json!({"length": 20, "width": 20, "height": 20, "body_name": "A"})).unwrap();
        h.app.run("solid.box", json!({"length": 20, "width": 20, "height": 20, "corner": [40, 0, 0], "body_name": "B"})).unwrap();
        h.app.run("solid.box", json!({"length": 20, "width": 20, "height": 20, "corner": [80, 0, 0], "body_name": "C"})).unwrap();
        h.app.run("construct.plane.offset", json!({"base": "XY", "offset": 20, "name": "Lid"})).unwrap();
        let sketch = h.app.run("sketch.create", json!({"plane": "Lid"})).unwrap()["sketch"].as_u64().unwrap_or_default();
        h.app.run("sketch.rectangle.two_point", json!({"p0": [5, 5], "p1": [15, 15]})).unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0": [45, 5], "p1": [55, 15]})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        h.app.run("browser.visibility", json!({"items": hide, "visible": false})).unwrap();
        h.app.session.selection = (0..2).map(|index| solidcraft_engine::Sel::Profile { sketch, index }).collect();
        h.app.start("solid.emboss");
        h
    };
    let mut found = Vec::new();
    for (deboss, hide) in [(false, &["B"][..]), (true, &["B"][..]), (false, &["A", "B", "C"][..])] {
        let mut h = setup(hide);
        let Some(mut d) = h.app.dialog.clone() else { panic!("no Emboss window") };
        let Kind::Emboss { deboss: mode, .. } = &mut d.kind else { panic!("{:?}", d.kind) };
        *mode = deboss;
        let cmds = match crate::dialogs::apply_commands(&h.app, &d) {
            Ok(cmds) => cmds,
            Err(e) => {
                found.push((deboss, hide.len(), volume(&h, "B").unwrap_or_default(), Some(json!(e)), None));
                continue;
            }
        };
        let sent = cmds.first().map(|(_, p)| p["targets"].clone());
        for (cmd, p) in cmds {
            h.app.run(&cmd, p).unwrap();
        }
        // Editing it keeps the shown boxes as its targets.
        let id = h.app.session.doc.features.last().map(|f| f.id);
        let edit = id.and_then(|id| crate::dialogs::for_feature(&h.app, id, None)).and_then(|d| crate::dialogs::apply_commands(&h.app, &d).ok());
        let kept = edit.as_ref().and_then(|c| c.first()).map(|(_, p)| p["params"]["targets"].clone());
        found.push((deboss, hide.len(), volume(&h, "B").unwrap_or_default(), sent, kept));
    }
    // The square over the hidden B meets no shown body. Joined, it goes into A as a separate
    // piece, as any Join tool that meets none of its targets does; that rule is not this test's.
    let refusal = json!(solidcraft_ui_text::tr("dialog.no_shown_body_show_one").to_string());
    for (deboss, hidden, b, sent, kept) in &found {
        let ok = if *hidden == 1 {
            let shown = Some(json!(["A", "C"]));
            (b - 8000.0).abs() < 1e-6 && *sent == shown && *kept == shown
        } else {
            (b - 8000.0).abs() < 1e-6 && *sent == Some(refusal.clone())
        };
        assert!(ok, "deboss={deboss}: the hidden B untouched, the shown boxes are the targets and stay so, nothing shown is refused: {found:?}");
    }
    // A Target Face on A, with B hidden: A stays the one target, not every shown box.
    let mut h = setup(&["B"]);
    if let Some(d) = h.app.dialog.as_mut()
        && let Some(input) = d.inputs.get_mut(1)
    {
        input.items = vec![solidcraft_engine::Sel::Face { body: "A".into(), index: 0, point: Vec3::new(10.0, 10.0, 20.0) }];
    }
    let cmds = h.app.dialog.as_ref().map(|d| crate::dialogs::apply_commands(&h.app, d));
    let targets = cmds.as_ref().and_then(|c| c.as_ref().ok()).and_then(|c| c.first()).map(|(_, p)| p["targets"].clone());
    assert_eq!(targets, Some(json!(["A"])), "a Target Face keeps its body as the one target: {cmds:?}");
}

/// A saved New Body feature changed to Cut in its edit window leaves a hidden body alone, as a
/// new Cut does: two 20 mm boxes, the second hidden, and a tool body that runs through both.
#[test]
fn a_new_body_edited_to_cut_leaves_hidden_bodies_alone() {
    let volume = |h: &Harness, name: &str| {
        let st = crate::open_meshing::session_world(&h.app.session);
        st.body(name).and_then(|b| solidcraft_engine::doc::kernel::measure(&b.body).ok()).map(|m| m.volume)
    };
    let mut found = Vec::new();
    for tool in ["solid.box", "solid.revolve", "solid.pipe", "surface.thicken"] {
        let mut h = Harness::new();
        h.app.run("solid.box", json!({"length": 20, "width": 20, "height": 20, "body_name": "A"})).unwrap();
        h.app.run("solid.box", json!({"length": 20, "width": 20, "height": 20, "corner": [40, 0, 0], "body_name": "B"})).unwrap();
        h.app.run("construct.plane.offset", json!({"base": "XY", "offset": 10, "name": "Mid"})).unwrap();
        h.app.run("sketch.create", json!({"plane": "Mid", "name": "S"})).unwrap();
        h.app.run("sketch.line", json!({"points": [[-10, 10], [70, 10]], "ids": ["path"]})).unwrap();
        if tool == "solid.revolve" {
            h.app.run("sketch.rectangle.two_point", json!({"p0": [5, 12], "p1": [55, 14]})).unwrap();
        }
        h.app.run("sketch.finish", json!({})).unwrap();
        let mut made = match tool {
            "solid.box" => json!({"length": 40, "width": 10, "height": 10, "corner": [10, 5, 5]}),
            "solid.revolve" => json!({"sketch": "S", "axis": "path"}),
            "solid.pipe" => json!({"path_sketch": "S", "path": ["path"], "diameter": 6}),
            _ => {
                h.app.run("surface.extrude", json!({"curves": ["path"], "distance": 4, "body_name": "Sheet"})).unwrap();
                json!({"bodies": ["Sheet"], "thickness": 2})
            }
        };
        made["operation"] = json!("new");
        made["name"] = json!("Tool");
        h.app.run(tool, made).unwrap_or_else(|e| panic!("{tool}: {e}"));
        h.app.run("browser.visibility", json!({"items": ["B"], "visible": false})).unwrap();
        h.app.run("select.clear", json!({})).unwrap();
        h.step(&json!({"call": "ui.editFeature", "params": {"feature": "Tool"}})).unwrap();
        h.frames(2);
        match h.app.dialog.as_mut().map(|d| &mut d.kind) {
            Some(
                Kind::Primitive { operation, .. }
                | Kind::Revolve { operation, .. }
                | Kind::Pipe { operation, .. }
                | Kind::Surface(crate::dialogs_surface::Surf::Thicken { operation, .. }),
            ) => *operation = CUT,
            other => panic!("{tool}: {other:?}"),
        }
        h.ctx.data_mut(|m| m.insert_temp(super::picked_id(), true));
        ok(&mut h);
        found.push((tool, volume(&h, "A").unwrap_or_default(), volume(&h, "B").unwrap_or_default()));
    }
    // Box: 10 x 10 x 10 of each box. Revolve: a ring of radii 2 and 4 over 15 mm, 180π
    // (565.49 mm3). Pipe: 6 mm across 20 mm, 180π. Thicken: 20 x 4 x 2.
    for (tool, a, b) in &found {
        let cut = match *tool {
            "solid.box" => 1000.0,
            "surface.thicken" => 160.0,
            _ => 180.0 * std::f64::consts::PI,
        };
        assert!((a - (8000.0 - cut)).abs() < 1.0 && (b - 8000.0).abs() < 1e-6, "{tool}: the shown box A is cut, the hidden B untouched: {found:?}");
    }
}

/// The same edit to Join joins only the shown box; with every body hidden it is refused and changes
/// nothing; a saved Cut that named no bodies keeps naming none when edited after a body is hidden.
#[test]
fn a_new_body_edited_to_join_or_with_every_body_hidden_leaves_hidden_bodies_alone() {
    let volume = |h: &Harness, name: &str| {
        let st = crate::open_meshing::session_world(&h.app.session);
        st.body(name).and_then(|b| solidcraft_engine::doc::kernel::measure(&b.body).ok()).map(|m| m.volume)
    };
    // Two 20 mm boxes and a 40 x 10 x 10 tool box through both, made as `operation`.
    let design = |operation: &str| {
        let mut h = Harness::new();
        h.app.run("solid.box", json!({"length": 20, "width": 20, "height": 20, "body_name": "A"})).unwrap();
        h.app.run("solid.box", json!({"length": 20, "width": 20, "height": 20, "corner": [40, 0, 0], "body_name": "B"})).unwrap();
        let tool = json!({"length": 40, "width": 10, "height": 10, "corner": [10, 5, 5], "operation": operation, "name": "Tool", "body_name": "T"});
        h.app.run("solid.box", tool).unwrap();
        h
    };
    let edit = |h: &mut Harness, to: Option<usize>| {
        h.app.run("select.clear", json!({})).unwrap();
        h.step(&json!({"call": "ui.editFeature", "params": {"feature": "Tool"}})).unwrap();
        h.frames(2);
        if let Some(to) = to {
            pick(h, to);
        }
    };
    // Join: A takes the tool (8000 + 4000 - 1000), the hidden B is untouched.
    let mut h = design("new");
    h.app.run("browser.visibility", json!({"items": ["B"], "visible": false})).unwrap();
    edit(&mut h, Some(JOIN));
    ok(&mut h);
    let joined = (volume(&h, "A").unwrap_or_default(), volume(&h, "B").unwrap_or_default());
    assert!((joined.0 - 11000.0).abs() < 1e-6 && (joined.1 - 8000.0).abs() < 1e-6, "A joined, the hidden B untouched: {joined:?}");
    // Every body hidden: OK is refused with the window's message and the design stays as it was.
    let mut h = design("new");
    h.app.run("browser.visibility", json!({"items": ["A", "B"], "visible": false})).unwrap();
    edit(&mut h, Some(CUT));
    let refused = h.app.dialog.as_ref().map(|d| crate::dialogs::apply_commands(&h.app, d));
    assert_eq!(refused, Some(Err(solidcraft_ui_text::tr("dialog.no_shown_body").to_string())));
    h.step(&json!({"key": "Escape"})).unwrap();
    h.frames(2);
    let kept = [volume(&h, "A"), volume(&h, "B"), volume(&h, "T")];
    assert!(
        h.app.dialog.is_none() && kept.iter().zip([8000.0, 8000.0, 4000.0]).all(|(v, want)| v.is_some_and(|v| (v - want).abs() < 1e-6)),
        "{kept:?}"
    );
    // A saved Cut that named no bodies (both were shown) keeps naming none when edited with B hidden.
    let mut h = design("cut");
    h.app.run("browser.visibility", json!({"items": ["B"], "visible": false})).unwrap();
    edit(&mut h, None);
    let sent = h.app.dialog.as_ref().and_then(|d| crate::dialogs::apply_commands(&h.app, d).ok());
    let targets = sent.as_ref().and_then(|c| c.first()).map(|(_, p)| p["params"].get("targets").cloned());
    assert_eq!(targets, Some(None), "the saved Cut keeps its saved targets: {sent:?}");
}

#[test]
fn into_a_hidden_body_with_nothing_shown_is_a_new_body_until_it_is_shown() {
    // The profile on the hidden cube's base, extruded into it (#9919).
    let mut h = cube();
    h.app.run("browser.visibility", json!({"items": ["Base"], "visible": false})).unwrap();
    rect_sketch(&mut h, [0.0, 0.0], [10.0, 10.0]);
    h.step(&json!({"start": "solid.extrude"})).unwrap();
    checked(&mut h);
    assert_eq!(op(&mut h), NEW, "nothing shown to cut");
    let preview = h.app.dialog.as_ref().map(|d| crate::dialogs::apply_commands(&h.app, d));
    assert!(preview.is_some_and(|c| c.is_ok_and(|c| !c.is_empty())), "the extrude runs");
    // Shown while the window is open: the extrude goes into it, a Cut.
    h.app.run("browser.visibility", json!({"items": ["Base"], "visible": true})).unwrap();
    checked(&mut h);
    assert_eq!(op(&mut h), CUT, "the shown cube is cut");
    // Hidden again: New Body again.
    h.app.run("browser.visibility", json!({"items": ["Base"], "visible": false})).unwrap();
    checked(&mut h);
    assert_eq!(op(&mut h), NEW);
    ok(&mut h);
    assert_eq!(bodies(&h).len(), 2, "{:?}", bodies(&h));
}

#[test]
fn an_edited_feature_keeps_its_operation() {
    let mut h = cube();
    rect_sketch(&mut h, [30.0, 0.0], [40.0, 10.0]);
    h.app.run("solid.extrude", json!({"distance": 5, "operation": "join", "name": "Apart"})).unwrap();
    h.step(&json!({"call": "ui.editFeature", "params": {"feature": "Apart"}})).unwrap();
    checked(&mut h);
    assert_eq!(op(&mut h), JOIN, "the saved Join stays");
}

/// Two 10 mm cubes that a 4 mm box window set to Cut reaches: `A` in the root component and `B` in
/// `Plate` (#20647). With `plate` false both are in the root.
fn two_cubes(plate: bool) -> Harness {
    let mut h = Harness::new();
    if plate {
        h.app.run("component.create", json!({"name": "Plate"})).unwrap();
        h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [1, 1, -5], "body_name": "B", "operation": "new"})).unwrap();
        h.app.run("component.activate", json!({"component": 0})).unwrap();
    } else {
        h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [1, 1, -5], "body_name": "B", "operation": "new"})).unwrap();
    }
    h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [1, 1, 1], "body_name": "A", "operation": "new"})).unwrap();
    h.app.start("solid.box");
    h.frames(2);
    if let Some(d) = h.app.dialog.as_mut()
        && let Kind::Primitive { fields, operation, .. } = &mut d.kind
    {
        for (_, value) in fields.iter_mut() {
            *value = "4".into();
        }
        *operation = CUT;
        d.op_auto.manual = true;
    }
    h.frames(2);
    h
}

fn cut_volume(h: &Harness, body: &str) -> f64 {
    h.app.session.world_state().unwrap().body(body).unwrap().measure().unwrap().volume
}

/// Runs the window's commands and returns the first one's parameters.
fn run_window(h: &mut Harness) -> serde_json::Value {
    let commands = crate::dialogs::apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    let (cmd, p) = commands.first().cloned().unwrap();
    h.app.run(&cmd, p.clone()).unwrap();
    p
}

#[test]
fn a_cut_across_two_components_cuts_every_ticked_body() {
    let mut h = two_cubes(true);
    if let Some(d) = h.app.dialog.as_mut() {
        d.extra.insert(crate::cut_bodies::SKIPPED.into(), json!(["A", "B"]));
    }
    let p = run_window(&mut h);
    let mut targets: Vec<&str> = p["targets"].as_array().unwrap().iter().filter_map(|t| t.as_str()).collect();
    targets.sort();
    assert_eq!(targets, ["A", "B"], "{p}");
    assert!((cut_volume(&h, "A") - 973.0).abs() < 0.5, "A lost the 3x3x3 the box reaches: {}", cut_volume(&h, "A"));
    assert!((cut_volume(&h, "B") - 964.0).abs() < 0.5, "B lost the 3x3x4 the box reaches: {}", cut_volume(&h, "B"));
}

#[test]
fn a_cut_across_two_components_leaves_an_unticked_body_alone() {
    let mut h = two_cubes(true);
    if let Some(d) = h.app.dialog.as_mut() {
        d.extra.insert(crate::cut_bodies::SKIPPED.into(), json!(["A"]));
    }
    let p = run_window(&mut h);
    assert_eq!(p["targets"], json!(["A"]), "{p}");
    assert!(p.get(crate::cut_bodies::SKIPPED).is_none(), "the list is not a command parameter: {p}");
    assert!((cut_volume(&h, "A") - 973.0).abs() < 0.5);
    assert!((cut_volume(&h, "B") - 1000.0).abs() < 0.5, "B was not ticked: {}", cut_volume(&h, "B"));
}

#[test]
fn a_cut_with_every_body_in_one_component_sends_no_list() {
    let mut h = two_cubes(false);
    let commands = crate::dialogs::apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    assert!(commands.iter().all(|(_, p)| p.get("targets").is_none()), "{commands:?}");
    h.frames(1);
}

/// An untouched list sends nothing of its own: the Cut takes the bodies it took before the list,
/// the shown scope or, without one, what the command does by itself (#20647).
#[test]
fn an_untouched_list_leaves_the_cut_as_it_was() {
    let h = two_cubes(true);
    let commands = crate::dialogs::apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    let (_, p) = commands.first().cloned().unwrap();
    match solidcraft_engine::shown_scope(&h.app.session) {
        Some(shown) => assert_eq!(p["targets"], json!(shown), "{p}"),
        None => assert!(p.get("targets").is_none(), "{p}"),
    }
}

/// A changed list holds when its only ticked body is hidden while the window is open: the Cut is
/// refused rather than run on the bodies the user did not tick (#20647).
#[test]
fn hiding_the_only_ticked_body_refuses_the_cut() {
    let mut h = two_cubes(true);
    if let Some(d) = h.app.dialog.as_mut() {
        d.extra.insert(crate::cut_bodies::SKIPPED.into(), json!(["B"]));
    }
    h.app.session.visibility.hidden_bodies.push("B".into());
    h.frames(1);
    assert!(crate::dialogs::apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).is_err());
}

/// Hiding an unticked body keeps the ticked one cut, even when that leaves one component shown and
/// the list with it (#20647).
#[test]
fn hiding_an_unticked_body_still_cuts_the_ticked_one() {
    let mut h = two_cubes(true);
    if let Some(d) = h.app.dialog.as_mut() {
        d.extra.insert(crate::cut_bodies::SKIPPED.into(), json!(["A"]));
    }
    h.app.session.visibility.hidden_bodies.push("B".into());
    h.frames(1);
    let commands = crate::dialogs::apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    assert_eq!(commands.first().map(|(_, p)| p["targets"].clone()), Some(json!(["A"])), "{commands:?}");
}
