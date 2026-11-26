//! The Offset Curve dialog (#22414): it opens from the SURFACE tab's command in a sketch, builds
//! the parameters `surface.offset_curve` takes, makes the same link the command makes, shows the
//! command's refusal and leaves the design as it was, and reopens on an offset curve with its
//! stored values, OK changing that link in place.

use serde_json::{Value, json};
use solidcore_record::Vec3;
use solidcraft_engine::Sel;
use solidcraft_engine::geom::prelude::*;
use solidcraft_engine::sketch::LinkSource;

use super::*;
use crate::dialogs::{Kind, apply_commands};
use crate::dialogs_surface::Surf;
use crate::scenario::{self, Harness};

/// A 40 × 30 plate `Plate` on XY, a sketch `Mark` with the line x = 20 across it, and an XZ
/// sketch `Offsets` being edited. Returns the Mark sketch's id and its line's id.
fn plate() -> (Harness, u64, String) {
    let mut h = Harness::new();
    let run = |h: &mut Harness, c: &str, p: Value| h.app.run(c, p).unwrap_or_else(|e| panic!("{c}: {e}"));
    run(&mut h, "sketch.create", json!({"plane": "XY", "name": "Rect"}));
    run(&mut h, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]}));
    run(&mut h, "sketch.finish", json!({}));
    run(&mut h, "surface.patch", json!({"sketch": "Rect", "body_name": "Plate"}));
    run(&mut h, "sketch.create", json!({"plane": "XY", "name": "Mark"}));
    let mark = h.app.session.active_sketch.unwrap();
    let line = run(&mut h, "sketch.line", json!({"points": [[20, 0], [20, 30]]}))["curves"][0].as_str().unwrap().to_string();
    run(&mut h, "sketch.finish", json!({}));
    run(&mut h, "sketch.create", json!({"plane": "XZ", "name": "Offsets"}));
    h.settle();
    (h, mark, line)
}

/// Frames until the live preview (computed on a worker) has its result, then a few more to draw.
fn settle_preview(h: &mut Harness) {
    h.settle();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while h.app.preview.busy {
        assert!(std::time::Instant::now() < deadline, "the preview never finished");
        std::thread::sleep(std::time::Duration::from_millis(1));
        h.frame();
    }
    h.frames(4);
}

fn opts(h: &Harness) -> &OffsetCurveOpts {
    match h.app.dialog.as_ref().map(|d| &d.kind) {
        Some(Kind::Surface(Surf::OffsetCurve(o))) => o,
        other => panic!("not the Offset Curve dialog: {other:?}"),
    }
}

fn opts_mut(h: &mut Harness) -> &mut OffsetCurveOpts {
    match h.app.dialog.as_mut().map(|d| &mut d.kind) {
        Some(Kind::Surface(Surf::OffsetCurve(o))) => o,
        _ => panic!("not the Offset Curve dialog"),
    }
}

/// The face of the plate at `(x, y)` and its edge through `edge`, as clicks pick them.
fn picks(h: &Harness, (x, y): (f64, f64), from: Sel) -> Vec<Sel> {
    let face = crate::dialogs::face_sel_on(&h.app.session, Some("Plate"), Vec3::new(x, y, 0.0)).unwrap();
    vec![face, from]
}

fn edge(h: &Harness, p: Vec3) -> Sel {
    crate::dialogs::edge_sel_on(&h.app.session, Some("Plate"), p).unwrap()
}

fn set_picks(h: &mut Harness, sels: Vec<Sel>) {
    let d = h.app.dialog.as_mut().unwrap();
    for (input, sel) in d.inputs.iter_mut().zip(sels) {
        input.items = vec![sel];
    }
}

/// The links of the sketch being edited, as stored.
fn links(h: &Harness) -> Vec<(String, LinkSource, Vec<String>)> {
    let sid = h.app.session.active_sketch.unwrap();
    let sk = h.app.session.doc.sketch(sid).unwrap();
    sk.links.iter().map(|l| (l.id.clone(), l.source.clone(), sk.link_wires(&l.id).iter().map(|w| sk.wires[*w].id.clone()).collect())).collect()
}

/// The points of the sketch's 3D curve `wire`, in the world.
fn wire_points(h: &Harness, wire: &str) -> Vec<Vec3> {
    let sid = h.app.session.active_sketch.unwrap();
    let sk = h.app.session.doc.sketch(sid).unwrap();
    sk.wires[sk.wire_index(wire).unwrap()].pts.clone()
}

/// The dialog's rows and picks, its parameters, and OK: the same link `surface.offset_curve`
/// makes from the same parameters, and the deviation it reports on the status line.
#[test]
fn the_dialog_makes_the_link_the_command_makes() {
    let (mut h, _, _) = plate();
    h.app.start("surface.offset_curve");
    h.settle();
    assert_eq!(opts(&h).link, None);
    assert!(!opts(&h).flip && opts(&h).tolerance.is_empty());
    assert_eq!(h.app.dialog.as_ref().unwrap().inputs.iter().map(|i| i.label).collect::<Vec<_>>(), ["Face", FROM]);
    // Nothing picked: no parameters, and the reason says what to pick.
    let e = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap_err();
    assert_eq!(e, solidcraft_ui_text::tr("ui.offset_curve_pick_face"));
    let from = edge(&h, Vec3::new(20.0, 0.0, 0.0));
    let sels = picks(&h, (20.0, 15.0), from);
    set_picks(&mut h, sels);
    opts_mut(&mut h).distance = "7 mm".into();
    settle_preview(&mut h);
    for row in ["Face", FROM, "Distance", "Tolerance"] {
        assert!(scenario::rect_of(&format!("dialog:row:{row}")).is_some(), "row {row}");
    }
    let cmds = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    assert_eq!(cmds.len(), 1);
    let (cmd, p) = &cmds[0];
    assert_eq!(cmd, "surface.offset_curve");
    assert_eq!(p["body"], json!("Plate"));
    assert_eq!(p["distance"], json!(7.0));
    assert_eq!(p["from"]["body"], json!("Plate"));
    assert!(p.get("flip").is_none() && p.get("tolerance").is_none() && p.get("link").is_none(), "{p}");
    // The live preview builds it, with no refusal, and draws the new linked 3D curve.
    assert_eq!(h.app.preview.error, None);
    assert!(h.app.preview.sketches.iter().any(|s| s.wires.len() == 1), "the preview shows the curve");
    // The command run by hand with those parameters, in a copy of the design.
    let mut twin = h.app.session.scratch();
    let reply = twin.execute(cmd, p).unwrap();
    let st = twin.model.state();
    let by_hand = st.sketch(twin.active_sketch.unwrap()).unwrap().sketch.links[0].source.clone();
    drop(st);
    h.step(&json!({"click": {"handle": "dialog:ok"}})).unwrap();
    h.settle();
    assert!(h.app.dialog.is_none(), "OK made it: {:?}", h.app.status);
    let made = links(&h);
    assert_eq!(made.len(), 1);
    assert_eq!(made[0].1, by_hand, "the dialog stores what the command stores");
    assert!(matches!(&made[0].1, LinkSource::FaceOffset { distance, flip: false, tolerance: None, .. } if *distance == 7.0));
    // The bottom edge (y = 0), 7 in: the line y = 7 across the plate, 40 long.
    let pts = wire_points(&h, &made[0].2[0]);
    assert!(pts.iter().all(|q| (q.y - 7.0).abs() < 1e-9 && q.z.abs() < 1e-9), "{pts:?}");
    let xs = pts.iter().map(|q| q.x);
    assert!((xs.clone().fold(f64::INFINITY, f64::min)).abs() < 1e-9 && (xs.fold(f64::NEG_INFINITY, f64::max) - 40.0).abs() < 1e-9);
    // The status line gives the deviation the command reported (a straight offset: 0 to rounding).
    assert!(reply["deviation"].as_f64().unwrap() < 1e-9, "{reply}");
    let status = h.app.status.as_ref().map(|(s, ..)| s.clone()).unwrap_or_default();
    assert_eq!(status, status_of(&reply), "{status}");
    assert!(status.contains("0.000000"), "{status}");
}

fn status_of(reply: &Value) -> String {
    status(reply).unwrap()
}

/// A distance that runs off the face: the command's refusal is in the dialog, OK makes nothing,
/// and the design is as it was. A curve of the sketch being edited is refused before the command.
#[test]
fn a_refusal_shows_in_the_dialog_and_changes_nothing() {
    let (mut h, _, _) = plate();
    let before = h.app.session.doc.to_json();
    h.app.start("surface.offset_curve");
    h.settle();
    let from = edge(&h, Vec3::new(20.0, 0.0, 0.0));
    let sels = picks(&h, (20.0, 15.0), from);
    set_picks(&mut h, sels);
    // The plate is 30 across from that edge.
    opts_mut(&mut h).distance = "50 mm".into();
    settle_preview(&mut h);
    let why = h.app.preview.error.clone().unwrap_or_default();
    assert!(why.contains("runs off the face everywhere"), "{why}");
    assert_eq!(scenario::count_of("dialog:ok:enabled"), Some(0.0), "OK waits for a curve that builds");
    let d = h.app.dialog.clone().unwrap();
    let e = crate::dialogs::run_dialog(&mut h.app, &d).unwrap_err();
    assert!(e.contains("runs off the face everywhere"), "{e}");
    assert_eq!(h.app.session.doc.to_json(), before, "a refusal leaves the design as it was");
    assert!(links(&h).is_empty());
    // A curve of the sketch being edited cannot be what it is offset from.
    let own = h.app.run("sketch.line", json!({"points": [[0, 0], [10, 0]]})).unwrap()["curves"][0].as_str().unwrap().to_string();
    let sid = h.app.session.active_sketch.unwrap();
    let d = h.app.dialog.as_mut().unwrap();
    d.inputs[1].items = vec![Sel::ExternalSketchCurve { sketch: sid, id: own }];
    let e = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap_err();
    assert_eq!(e, solidcraft_ui_text::tr("ui.offset_curve_other_sketch"));
}

/// Edit Offset Curve: the selected 3D curve's link reopens the dialog with its face, source,
/// distance, tolerance and Flip; OK changes that link (same link and 3D curve ids).
#[test]
fn edit_reopens_the_offset_curve_and_ok_changes_it_in_place() {
    let (mut h, mark, line) = plate();
    // The line x = 20, 5 mm, the face picked at x = 10, flipped: x = 25.
    let made = h
        .app
        .run(
            "surface.offset_curve",
            json!({"face": [10, 15, 0], "body": "Plate", "from": {"sketch": mark, "curve": line}, "distance": 5, "tolerance": 0.001, "flip": true}),
        )
        .unwrap();
    h.settle();
    let (link, wire) = (made["link"].as_str().unwrap().to_string(), made["wires"][0].as_str().unwrap().to_string());
    assert!(wire_points(&h, &wire).iter().all(|q| (q.x - 25.0).abs() < 1e-9));
    h.app.session.selection = vec![Sel::SketchCurve { id: wire.clone(), sketch: None }];
    assert_eq!(selected(&h.app).as_deref(), Some(link.as_str()));
    let target = crate::context_menu::Target::Viewport;
    let menu: Vec<String> = crate::context_menu::entries(&h.app, &target).into_iter().map(|i| i.id).collect();
    assert!(menu.iter().any(|id| id == "ui.editOffsetCurve"), "{menu:?}");
    assert!(edit(&mut h.app, &link));
    h.settle();
    let o = opts(&h).clone();
    let doc = &h.app.session.doc;
    let mm = |t: &str| crate::length_input::eval(doc, t, ValueKind::Length).unwrap();
    assert_eq!((mm(&o.distance), mm(&o.tolerance), o.flip, o.link.as_deref()), (5.0, 0.001, true, Some(link.as_str())));
    let d = h.app.dialog.as_ref().unwrap();
    assert!(matches!(&d.inputs[0].items[..], [Sel::Face { body, point, .. }] if body == "Plate" && point.dist(Vec3::new(10.0, 15.0, 0.0)) < 1e-9));
    assert_eq!(d.inputs[1].items, vec![Sel::ExternalSketchCurve { sketch: mark, id: line.clone() }]);
    // OK as it stands sends back what the link holds.
    let (_, p) = apply_commands(&h.app, d).unwrap().remove(0);
    assert_eq!(
        (p["distance"].clone(), p["tolerance"].clone(), p["flip"].clone(), p["link"].clone()),
        (json!(5.0), json!(0.001), json!(true), json!(link))
    );
    // 8 mm: x = 28, in the same link and 3D curve.
    opts_mut(&mut h).distance = "8 mm".into();
    settle_preview(&mut h);
    // The changed curve is previewed (its link now reads 8 mm).
    assert_eq!(h.app.preview.error, None);
    assert!(h.app.preview.sketches.iter().any(|s| s.wires.len() == 1), "the preview shows the changed curve");
    h.step(&json!({"click": {"handle": "dialog:ok"}})).unwrap();
    h.settle();
    assert!(h.app.dialog.is_none(), "{:?}", h.app.status);
    let now = links(&h);
    assert_eq!(now.len(), 1, "an edit, not another offset curve");
    assert_eq!((now[0].0.as_str(), now[0].2.as_slice()), (link.as_str(), [wire.clone()].as_slice()));
    assert!(matches!(&now[0].1, LinkSource::FaceOffset { distance, flip: true, .. } if *distance == 8.0));
    let pts = wire_points(&h, &wire);
    assert!(pts.iter().all(|q| (q.x - 28.0).abs() < 1e-9 && q.z.abs() < 1e-9), "{pts:?}");
    let ys = pts.iter().map(|q| q.y);
    assert!((ys.clone().fold(f64::INFINITY, f64::min)).abs() < 1e-9 && (ys.fold(f64::NEG_INFINITY, f64::max) - 30.0).abs() < 1e-9);
}

/// The status line's words for a reply, and none for a reply without a deviation.
#[test]
fn the_status_line_gives_the_deviation_and_a_cut() {
    assert_eq!(
        status(&json!({"deviation": 0.00125, "clipped": false})).unwrap(),
        solidcraft_ui_text::format("ui.offset_curve_done", &[("deviation", "0.001250")])
    );
    assert_eq!(
        status(&json!({"deviation": 0.0, "clipped": true})).unwrap(),
        solidcraft_ui_text::format("ui.offset_curve_done_clipped", &[("deviation", "0.000000")])
    );
    assert_eq!(status(&json!({"link": "j1"})), None);
}
