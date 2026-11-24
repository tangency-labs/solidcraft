use crate::test_support::run_all;
use serde_json::json;
use solidcore_record::Vec3;
use solidcraft_engine::Session;

use super::*;
use crate::Services;
use crate::dialogs::{Dialog, apply_commands, for_feature as edit_dialog};

fn app_with(cmds: &[(&str, Value)]) -> SolveApp {
    let mut s = Session::default();
    for (id, p) in cmds {
        s.execute(id, p).unwrap();
    }
    SolveApp::new(s, Services::default())
}

/// A 600 × 400 rectangle on XY as the frame sketch `Frame`, and, drawn after it on YZ, a ring of
/// two circles as the section sketch `Ring` (its circles' ids are not the frame's line ids).
fn frame_app() -> SolveApp {
    app_with(&frame_commands())
}

fn frame_commands() -> Vec<(&'static str, Value)> {
    vec![
        ("sketch.create", json!({"plane": "XY", "name": "Frame"})),
        ("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [600, 400]})),
        ("sketch.finish", json!({})),
        ("sketch.create", json!({"plane": "YZ", "name": "Ring"})),
        ("sketch.circle.center", json!({"center": [0, 0], "radius": 20})),
        ("sketch.circle.center", json!({"center": [0, 0], "radius": 17})),
        ("sketch.finish", json!({})),
    ]
}

fn sketch_id(app: &SolveApp, name: &str) -> u64 {
    app.session.doc.find_feature(name).unwrap().id
}

fn frame_lines(app: &SolveApp) -> Vec<String> {
    app.session.doc.sketch(sketch_id(app, "Frame")).unwrap().curves.iter().map(|c| c.id.clone()).collect()
}

fn body_names(app: &SolveApp) -> Vec<String> {
    crate::open_meshing::session_world(&app.session).bodies.iter().map(|b| b.name.clone()).collect()
}

/// The dialog started with the given lines picked.
fn dialog_on(app: &mut SolveApp, lines: &[String]) -> Dialog {
    app.session.selection = lines.iter().map(|id| Sel::SketchCurve { id: id.clone(), sketch: None }).collect();
    app.start("frame.member");
    app.dialog.clone().unwrap()
}

fn member(d: &mut Dialog) -> &mut Member {
    match &mut d.kind {
        Kind::Frame(m) => m,
        k => panic!("{k:?}"),
    }
}

/// Picked lines and the default built-in profile make one member a line; Edit Feature refills
/// the dialog and runs `timeline.redefine`, and the members keep their names.
#[test]
fn members_from_picked_lines_and_a_built_in_profile() {
    let mut app = frame_app();
    let lines = frame_lines(&app);
    let mut d = dialog_on(&mut app, &lines[..2]);
    let c = apply_commands(&app, &d).unwrap();
    assert_eq!(c[0].0, "frame.member");
    let p = &c[0].1;
    assert_eq!(p["sketch"], json!(sketch_id(&app, "Frame")), "the frame sketch, not the newer section sketch: {p}");
    assert_eq!(p["lines"], json!(lines[..2]));
    assert_eq!(p["profile"], json!({"kind": "rect_tube", "width": "40 mm", "height": "40 mm", "wall": "3 mm"}));
    assert!(p.get("anchor").is_none() && p.get("angle").is_none() && p.get("mirror").is_none() && p.get("body_name").is_none(), "{p}");
    // Another built-in profile and its dimensions; an optional one left empty takes its default.
    let m = member(&mut d);
    m.kind = frame::profile_kinds().iter().position(|k| k.kind == "angle").unwrap();
    m.dims[m.kind][2] = "4 mm".into();
    let c = apply_commands(&app, &d).unwrap();
    assert_eq!(c[0].1["profile"], json!({"kind": "angle", "width": "50 mm", "height": "50 mm", "thickness": "4 mm"}));
    run_all(&mut app, c);
    assert_eq!(body_names(&app), ["Angle 50x50x4", "Angle 50x50x4 (2)"]);
    app.run("body.rename", json!({"body": "Angle 50x50x4 (2)", "name": "Rail"})).unwrap();
    let f = app.session.doc.features.last().unwrap().id;
    let mut e = edit_dialog(&app, f, None).unwrap();
    assert_eq!(e.inputs[0].items.len(), 2);
    let m = member(&mut e);
    assert_eq!((frame::profile_kinds()[m.kind].kind, m.dims[m.kind][2].as_str()), ("angle", "4 mm"));
    m.dims[m.kind][2] = "5 mm".into();
    let c = apply_commands(&app, &e).unwrap();
    assert_eq!(c[0].0, "timeline.redefine");
    run_all(&mut app, c);
    assert_eq!(body_names(&app), ["Angle 50x50x4", "Rail"], "the members keep their names through the edit");
}

/// A section picked in another sketch is the profile, the picks naming its regions; All Lines,
/// anchor, angle, mirror and name reach the command and come back when editing.
#[test]
fn a_picked_section_and_the_placement() {
    let mut app = frame_app();
    let lines = frame_lines(&app);
    let ring = sketch_id(&app, "Ring");
    let mut d = dialog_on(&mut app, &lines[..1]);
    // The ring between the circles: the region whose interior point lies between radii 17 and 20.
    let ss = crate::open_meshing::session_world(&app.session).sketch(ring).unwrap().clone();
    let index = ss.profiles.iter().position(|p| (p.region.interior_point().len() - 18.5).abs() < 1.5).unwrap();
    d.inputs[1].items = vec![Sel::Profile { sketch: ring, index }];
    let m = member(&mut d);
    m.all = true;
    m.anchor = ANCHORS.iter().position(|a| a.0 == "bottom").unwrap();
    m.angle = "15 deg".into();
    m.mirror = true;
    m.name = " Rail ".into();
    let c = apply_commands(&app, &d).unwrap();
    let p = &c[0].1;
    assert_eq!(p["sketch"], json!(sketch_id(&app, "Frame")));
    assert!(p.get("lines").is_none() && p.get("profile").is_none(), "{p}");
    assert_eq!((&p["profile_sketch"], &p["profiles"]), (&json!(ring), &json!([index])));
    assert_eq!((&p["anchor"], &p["angle"], &p["mirror"], &p["body_name"]), (&json!("bottom"), &json!("15 deg"), &json!(true), &json!("Rail")));
    run_all(&mut app, c);
    assert_eq!(body_names(&app), ["Rail", "Rail (2)", "Rail (3)", "Rail (4)"]);
    let f = app.session.doc.features.last().unwrap().id;
    let mut e = edit_dialog(&app, f, None).unwrap();
    assert_eq!(e.inputs[0].items.len(), 4, "every line of the sketch shows as picked");
    assert_eq!(e.inputs[1].items, [Sel::Profile { sketch: ring, index }]);
    let m = member(&mut e);
    assert!(m.all && m.mirror && m.name == "Rail" && m.angle == "15 deg" && ANCHORS[m.anchor].0 == "bottom", "{m:?}");
}

/// A member whose section is a whole profile sketch keeps it when edited: the dialog shows the
/// sketch rather than picking every region of it, which would fill the tube's hole.
#[test]
fn a_whole_sketch_section_stays_whole_through_an_edit() {
    let mut app = frame_app();
    app.run("frame.member", json!({"sketch": "Frame", "profile_sketch": "Ring"})).unwrap();
    let volume = |app: &mut SolveApp| app.session.execute("inspect.measure", &json!({})).unwrap()["bodies"][0]["volume_mm3"].as_f64().unwrap();
    let before = volume(&mut app);
    let f = app.session.doc.features.last().unwrap().id;
    let mut e = edit_dialog(&app, f, None).unwrap();
    assert!(e.inputs[1].items.is_empty());
    assert_eq!(member(&mut e).whole, Some(sketch_id(&app, "Ring")));
    member(&mut e).angle = "30 deg".into();
    let c = apply_commands(&app, &e).unwrap();
    assert!(c[0].1["params"].get("profiles").is_none(), "{}", c[0].1);
    run_all(&mut app, c);
    // To the accuracy SMLib measures at (1e-4 relative, the finest it takes: kernel
    // `measure::ACCURACY`); the ring's exact π (20² − 17²) 600 = 209230.07 mm³ came out
    // 209229.86 before the turn and 209230.43 after it. The filled circle would be 3.6 times it.
    let after = volume(&mut app);
    assert!((after - before).abs() < 1e-4 * before, "still the ring, not the filled circle: {after} mm³, {before} before");
    // Unticked, the section is a built-in profile again.
    let mut e = edit_dialog(&app, f, None).unwrap();
    member(&mut e).whole = None;
    assert_eq!(apply_commands(&app, &e).unwrap()[0].1["params"]["profile"]["kind"], "rect_tube");
}

/// A member placed by a profile point (`locate`, which the dialog has no field for) keeps it
/// through an edit, so it stays attached where it was.
#[test]
fn an_edit_keeps_a_located_point() {
    let mut app = frame_app();
    let profile = json!({"kind": "angle", "width": 60, "height": 40, "thickness": 5});
    app.run("frame.member", json!({"sketch": "Frame", "profile": profile, "locate": [10, 5]})).unwrap();
    let f = app.session.doc.features.last().unwrap().id;
    let mut e = edit_dialog(&app, f, None).unwrap();
    member(&mut e).angle = "90 deg".into();
    let c = apply_commands(&app, &e).unwrap();
    assert_eq!(c[0].1["params"]["locate"], json!([10.0, 5.0]), "{}", c[0].1);
    run_all(&mut app, c);
    let FeatureKind::FrameMember { locate, .. } = &app.session.doc.feature(f).unwrap().kind else { panic!() };
    assert_eq!(*locate, Some([10.0, 5.0]));
}

/// Each group keeps its own located point through an edit, while another group is shown and
/// after the edit runs.
#[test]
fn an_edit_keeps_each_groups_located_point() {
    let mut app = frame_app();
    let lines = frame_lines(&app);
    let profile = json!({"kind": "angle", "width": 60, "height": 40, "thickness": 5});
    let groups = json!([{"lines": [&lines[0]], "locate": [10, 5]}, {"lines": [&lines[1]], "angle": "90 deg"}]);
    app.run("frame.member", json!({"sketch": "Frame", "profile": profile, "groups": groups})).unwrap();
    let f = app.session.doc.features.last().unwrap().id;
    let mut e = edit_dialog(&app, f, None).unwrap();
    if let Kind::Frame(m) = &mut e.kind {
        m.show(1, &mut e.inputs);
        m.name = "Post".into();
        m.show(0, &mut e.inputs);
        assert_eq!(m.locate, Some([10.0, 5.0]));
    }
    let c = apply_commands(&app, &e).unwrap();
    let sent = &c[0].1["params"];
    assert!(sent.get("locate").is_none(), "{sent}");
    assert_eq!(sent["groups"][0]["locate"], json!([10.0, 5.0]), "{sent}");
    assert!(sent["groups"][1].get("locate").is_none(), "{sent}");
    run_all(&mut app, c);
    let groups = frame::member_groups(&app.session.doc.feature(f).unwrap().kind);
    assert_eq!(groups.iter().map(|g| g.locate).collect::<Vec<_>>(), [Some([10.0, 5.0]), None]);
    assert_eq!(groups[1].body_name.as_deref(), Some("Post"));
}

/// Without lines there is nothing to place; lines and the section in one sketch are refused.
#[test]
fn missing_or_mixed_picks_are_refused() {
    let mut app = frame_app();
    let d = dialog_on(&mut app, &[]);
    assert!(apply_commands(&app, &d).unwrap_err().contains("lines"));
    let ring = sketch_id(&app, "Ring");
    let circles: Vec<String> = app.session.doc.sketch(ring).unwrap().curves.iter().map(|c| c.id.clone()).collect();
    let mut d = dialog_on(&mut app, &circles[..1]);
    d.inputs[1].items = vec![Sel::Profile { sketch: ring, index: 0 }];
    assert!(apply_commands(&app, &d).is_err());
}

/// Lines picked in the view keep their sketch: two 3D sketches both number their lines `w1`…, and
/// the members go on the lines that were clicked, not on the newer sketch's lines of those ids.
#[test]
fn picked_lines_stay_in_the_sketch_they_were_picked_in() {
    let mut app = app_with(&[
        ("sketch.create", json!({"plane": "XY", "name": "Uprights"})),
        ("sketch.options", json!({"sketch_3d": true})),
        ("sketch.line3d", json!({"points": [[0, 0, 0], [0, 0, 500]]})),
        ("sketch.line3d", json!({"points": [[300, 0, 0], [300, 0, 500]]})),
        ("sketch.finish", json!({})),
        ("sketch.create", json!({"plane": "XY", "name": "Rails"})),
        ("sketch.options", json!({"sketch_3d": true})),
        ("sketch.line3d", json!({"points": [[0, 0, 250], [300, 0, 250]]})),
        ("sketch.line3d", json!({"points": [[0, 0, 480], [300, 0, 480]]})),
        ("sketch.finish", json!({})),
    ]);
    let (uprights, rails) = (sketch_id(&app, "Uprights"), sketch_id(&app, "Rails"));
    let wires = |id: u64| -> Vec<String> { app.session.doc.sketch(id).unwrap().wires.iter().map(|w| w.id.clone()).collect() };
    let ids = wires(uprights);
    assert_eq!(ids, wires(rails), "the two sketches share their line ids");
    app.start("frame.member");
    let mut d = app.dialog.clone().unwrap();
    d.inputs[0].items = ids.iter().map(|id| Sel::ExternalSketchCurve { sketch: uprights, id: id.clone() }).collect();
    let c = apply_commands(&app, &d).unwrap();
    assert_eq!(c[0].1["sketch"], json!(uprights));
    run_all(&mut app, c.clone());
    // Two posts the height of the uprights, not two rails.
    let m = app.session.execute("inspect.measure", &json!({})).unwrap();
    for b in m["bodies"].as_array().unwrap() {
        let (lo, hi) = (b["bbox"]["min"][2].as_f64().unwrap(), b["bbox"]["max"][2].as_f64().unwrap());
        assert!(lo.abs() < 1e-6 && (hi - 500.0).abs() < 1e-6, "{b}");
    }
    // Lines picked in two sketches are refused.
    d.inputs[0].items.push(Sel::ExternalSketchCurve { sketch: rails, id: ids[0].clone() });
    assert!(apply_commands(&app, &d).unwrap_err().contains("one sketch"));
}

/// Lines clicked in two sketches on XY, one 300 mm above the other: the first sketch's line is
/// the path's line, the other sketch's goes in as its two end points, a pair, so neither click
/// is refused. A 3D sketch's line goes in the same way.
#[test]
fn a_line_clicked_in_another_sketch_goes_in_as_its_two_end_points() {
    let mut app = app_with(&[
        ("sketch.create", json!({"plane": "XY", "name": "Low"})),
        ("sketch.line", json!({"points": [[0, 0], [500, 0]]})),
        ("sketch.finish", json!({})),
        ("sketch.create", json!({"plane": "XY", "offset": 300, "name": "High"})),
        ("sketch.line", json!({"points": [[0, 0], [500, 0]]})),
        ("sketch.finish", json!({})),
        ("sketch.create", json!({"plane": "XY", "name": "Post"})),
        ("sketch.options", json!({"sketch_3d": true})),
        ("sketch.line3d", json!({"points": [[0, 0, 0], [0, 0, 300]]})),
        ("sketch.finish", json!({})),
    ]);
    let (low, high, post) = (sketch_id(&app, "Low"), sketch_id(&app, "High"), sketch_id(&app, "Post"));
    let line = |sketch: u64| {
        let sk = app.session.doc.sketch(sketch).unwrap();
        let id = sk.curves.first().map(|c| c.id.clone()).or_else(|| sk.wires.first().map(|w| w.id.clone())).unwrap();
        Sel::ExternalSketchCurve { sketch, id }
    };
    let (l_low, l_high, l_post) = (line(low), line(high), line(post));
    let mut d = dialog_on(&mut app, &[]);
    pick_all(&app, &mut d, &[l_low.clone(), l_high, l_post]);
    assert_eq!(d.error, None);
    let items = path(&d);
    assert_eq!(items.len(), 5, "{items:?}");
    assert_eq!(items[0], l_low);
    assert!(items[1..].iter().all(|x| matches!(x, Sel::SketchPoint { .. })), "{items:?}");
    let c = apply_commands(&app, &d).unwrap();
    assert_eq!(c[0].1["sketch"], json!(low));
    let pairs = c[0].1["pairs"].as_array().unwrap();
    assert_eq!(pairs.len(), 2);
    assert_eq!(pairs[0]["a"]["sketch"], json!(high));
    assert_eq!(pairs[1]["b"]["sketch"], json!(post));
    run_all(&mut app, c);
    assert_eq!(members(&app).len(), 3);
    // The high member lies 300 mm up, the post stands from 0 to 300.
    let m = app.session.execute("inspect.measure", &json!({})).unwrap();
    let zs: Vec<(f64, f64)> =
        m["bodies"].as_array().unwrap().iter().map(|b| (b["bbox"]["min"][2].as_f64().unwrap(), b["bbox"]["max"][2].as_f64().unwrap())).collect();
    assert!(zs.iter().any(|(lo, hi)| (lo + hi) / 2.0 > 290.0 && (hi - lo) < 100.0), "{zs:?}");
}

// ---------------------------------------------------------------------------------------------
// Members between pairs of points (#15705)

/// A 600 × 400 × 400 box `Box` extruded from the rectangle `Base` on XY: its corners are points to
/// pick, and `Base` keeps its lines.
fn box_app() -> SolveApp {
    app_with(&[
        ("sketch.create", json!({"plane": "XY", "name": "Base"})),
        ("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [600, 400]})),
        ("sketch.finish", json!({})),
        ("solid.extrude", json!({"distance": 400, "name": "Box", "body_name": "Box"})),
    ])
}

fn corner(x: f64, y: f64, z: f64) -> Sel {
    Sel::Vertex { body: "Box".into(), point: Vec3::new(x, y, z) }
}

/// A body vertex as the command takes it from a pick.
fn vertex(x: f64, y: f64, z: f64) -> Value {
    json!({"vertex": [x, y, z]})
}

/// The bodies Frame Member made (all but the box).
fn members(app: &SolveApp) -> Vec<String> {
    body_names(app).into_iter().filter(|n| n != "Box").collect()
}

/// Each pick goes through the path's picking as a click in the view does.
fn pick_all(app: &SolveApp, d: &mut Dialog, picks: &[Sel]) {
    for p in picks {
        pick(app, d, p.clone());
    }
}

fn path(d: &Dialog) -> &[Sel] {
    &d.inputs[0].items
}

#[test]
fn four_points_make_two_separate_members() {
    let mut app = box_app();
    let mut d = dialog_on(&mut app, &[]);
    assert_eq!(d.inputs[0].label, "Path");
    pick_all(&app, &mut d, &[corner(0., 0., 0.), corner(0., 0., 400.), corner(600., 0., 0.), corner(600., 0., 400.)]);
    assert_eq!(path(&d).len(), 4);
    assert_eq!(d.error, None);
    let c = apply_commands(&app, &d).unwrap();
    let p = &c[0].1;
    assert!(p.get("sketch").is_none() && p.get("lines").is_none(), "{p}");
    assert_eq!(p["pairs"], json!([{"a": vertex(0., 0., 0.), "b": vertex(0., 0., 400.)}, {"a": vertex(600., 0., 0.), "b": vertex(600., 0., 400.)}]));
    run_all(&mut app, c);
    assert_eq!(members(&app).len(), 2);
}

/// p0, p1, p1, p2: the corner picked twice is where two members meet.
#[test]
fn a_shared_point_picked_twice_joins_two_members() {
    let mut app = box_app();
    let mut d = dialog_on(&mut app, &[]);
    pick_all(&app, &mut d, &[corner(0., 0., 0.), corner(0., 0., 400.), corner(0., 0., 400.), corner(600., 0., 400.)]);
    assert_eq!(path(&d).len(), 4, "a point picked again goes in again");
    let c = apply_commands(&app, &d).unwrap();
    assert_eq!(
        c[0].1["pairs"],
        json!([{"a": vertex(0., 0., 0.), "b": vertex(0., 0., 400.)}, {"a": vertex(0., 0., 400.), "b": vertex(600., 0., 400.)}])
    );
    run_all(&mut app, c);
    assert_eq!(members(&app).len(), 2);
}

#[test]
fn a_lone_last_point_waits_with_pick_the_second_point() {
    let mut app = box_app();
    let mut d = dialog_on(&mut app, &[]);
    pick(&app, &mut d, corner(0., 0., 0.));
    assert_eq!(d.error.as_deref(), Some("Pick the second point"));
    pick_all(&app, &mut d, &[corner(0., 0., 400.), corner(600., 0., 0.)]);
    assert_eq!(d.error.as_deref(), Some("Pick the second point"));
    // OK makes the members already paired; the lone point waits.
    let c = apply_commands(&app, &d).unwrap();
    assert_eq!(c[0].1["pairs"], json!([{"a": vertex(0., 0., 0.), "b": vertex(0., 0., 400.)}]));
    pick(&app, &mut d, corner(600., 0., 400.));
    assert_eq!(d.error, None, "paired: nothing waits");
    let c = apply_commands(&app, &d).unwrap();
    run_all(&mut app, c);
    assert_eq!(members(&app).len(), 2);
}

#[test]
fn a_line_clicked_while_a_point_waits_unpicks_the_point() {
    let mut app = box_app();
    let base = sketch_id(&app, "Base");
    let line = Sel::ExternalSketchCurve { sketch: base, id: app.session.doc.sketch(base).unwrap().curves[0].id.clone() };
    let mut d = dialog_on(&mut app, &[]);
    pick(&app, &mut d, corner(0., 0., 0.));
    pick(&app, &mut d, line.clone());
    assert_eq!((path(&d), d.error.as_deref()), (&[line.clone()][..], None), "the line takes the waiting point's place");
    // After a whole pair the pair stays.
    pick_all(&app, &mut d, &[corner(0., 0., 0.), corner(0., 0., 400.), corner(600., 0., 400.)]);
    let other = Sel::ExternalSketchCurve { sketch: base, id: app.session.doc.sketch(base).unwrap().curves[1].id.clone() };
    pick(&app, &mut d, other.clone());
    assert_eq!(path(&d), [line.clone(), corner(0., 0., 0.), corner(0., 0., 400.), other]);
    // The command itself still refuses a point followed by a line.
    d.inputs[0].items = vec![corner(0., 0., 0.), line.clone()];
    assert_eq!(apply_commands(&app, &d).unwrap_err(), "A point needs another point after it");
    d.inputs[0].items = vec![line.clone(), corner(0., 0., 0.), corner(0., 0., 400.)];
    let c = apply_commands(&app, &d).unwrap();
    assert_eq!((&c[0].1["sketch"], c[0].1["pairs"].as_array().map(Vec::len)), (&json!(base), Some(1)));
}

#[test]
fn clicking_the_waiting_point_again_unpicks_it() {
    let mut app = box_app();
    let mut d = dialog_on(&mut app, &[]);
    pick(&app, &mut d, corner(0., 0., 0.));
    assert_eq!(d.error.as_deref(), Some("Pick the second point"));
    pick(&app, &mut d, corner(0., 0., 0.));
    assert_eq!((path(&d), d.error.as_deref()), (&[][..], None), "the point is unpicked and nothing waits");
    // After a whole pair, its last point clicked again starts the next member from it.
    pick_all(&app, &mut d, &[corner(0., 0., 0.), corner(0., 0., 400.), corner(0., 0., 400.)]);
    assert_eq!(path(&d).len(), 3);
    assert_eq!(d.error.as_deref(), Some("Pick the second point"));
    // And that waiting point, clicked again, goes; its pair stays.
    pick(&app, &mut d, corner(0., 0., 400.));
    assert_eq!(path(&d), [corner(0., 0., 0.), corner(0., 0., 400.)]);
}

#[test]
fn a_point_can_be_picked_again_and_ctrl_click_takes_out_its_last_occurrence() {
    let mut app = box_app();
    let mut d = dialog_on(&mut app, &[]);
    let (p0, p1, p2) = (corner(0., 0., 0.), corner(0., 0., 400.), corner(600., 0., 400.));
    pick_all(&app, &mut d, &[p0.clone(), p1.clone(), p1.clone(), p2.clone()]);
    app.dialog = Some(d);
    assert!(take_out(&mut app, &p1));
    let d = app.dialog.clone().unwrap();
    assert_eq!(path(&d), [p0.clone(), p1.clone(), p2.clone()], "the later p1 goes");
    assert_eq!(d.error.as_deref(), Some("Pick the second point"), "p2 now waits");
    assert_eq!(apply_commands(&app, &d).unwrap()[0].1["pairs"], json!([{"a": vertex(0., 0., 0.), "b": vertex(0., 0., 400.)}]));
    // A point not in the path, or a line, is no Ctrl+click take-out: it goes to the input as usual.
    assert!(!take_out(&mut app, &corner(600., 400., 0.)));
    let line = Sel::ExternalSketchCurve { sketch: sketch_id(&app, "Base"), id: "l1".into() };
    assert!(!take_out(&mut app, &line));
}

#[test]
fn vertex_construction_point_sketch_point_and_midpoint_picks_all_make_pairs() {
    use solidcraft_engine::snap::{SnapKind, SnapPoint};
    let mut app = box_app();
    app.run("construct.point.vertex", json!({"point": {"point": [600, 400, 600]}, "name": "Apex"})).unwrap();
    for (id, p) in [
        ("sketch.create", json!({"plane": "XY", "name": "Marks"})),
        ("sketch.point", json!({"point": [0, -100], "id": "m1"})),
        ("sketch.finish", json!({})),
    ] {
        app.run(id, p).unwrap();
    }
    let marks = sketch_id(&app, "Marks");
    // The middle of the box's top front edge, woken by hovering the edge.
    let middle = Vec3::new(300.0, 0.0, 400.0);
    let edge = Sel::Edge { body: "Box".into(), index: 0, point: Vec3::new(100.0, 0.0, 400.0) };
    app.viewport.snapped = vec![(edge, SnapPoint { at: middle, kind: SnapKind::Middle, body: "Box".into() })];
    let mut d = dialog_on(&mut app, &[]);
    pick_all(
        &app,
        &mut d,
        &[
            corner(0., 0., 0.),
            Sel::ConstructionPoint { name: "Apex".into() },
            Sel::SketchPoint { id: format!("{marks}:m1") },
            Sel::Vertex { body: "Box".into(), point: middle },
        ],
    );
    assert_eq!((path(&d).len(), d.error.as_deref()), (4, None));
    assert!(member(&mut d).refs.iter().all(Option::is_some), "each point's reference is fixed as it is picked");
    // The woken middle is forgotten (256 woken points later, in a long session): it stays the
    // middle, not the vertex nearest it.
    app.viewport.snapped.clear();
    let c = apply_commands(&app, &d).unwrap();
    let pairs = &c[0].1["pairs"];
    assert_eq!(pairs[0], json!({"a": vertex(0., 0., 0.), "b": {"construct_point": "Apex"}}));
    assert_eq!(pairs[1]["a"], json!({"sketch": marks, "point": "m1"}));
    assert_eq!((&pairs[1]["b"]["type"], &pairs[1]["b"]["kind"]), (&json!("snap"), &json!("middle")), "{pairs}");
    run_all(&mut app, c);
    assert_eq!(members(&app).len(), 2);
}

/// Edit Feature refills the path with the lines, then each pair's points in order, keeping the
/// references they were saved as: OK unchanged gives the pairs back exactly, ids and all.
#[test]
fn edit_feature_refills_lines_and_pair_points_in_order_and_keeps_their_refs() {
    let mut app = box_app();
    let base = sketch_id(&app, "Base");
    let l0 = app.session.doc.sketch(base).unwrap().curves[0].id.clone();
    let upright = json!({"a": vertex(600., 0., 0.), "b": vertex(600., 0., 400.)});
    let rail = json!({"a": vertex(0., 0., 400.), "b": vertex(600., 0., 400.)});
    app.run(
        "frame.member",
        json!({"sketch": "Base", "lines": [l0], "profile": {"kind": "rect_tube", "width": 40, "height": 40, "wall": 3}, "pairs": [upright, rail]}),
    )
    .unwrap();
    let f = app.session.doc.features.last().unwrap().id;
    let stored = |app: &SolveApp| match &app.session.doc.feature(f).unwrap().kind {
        FeatureKind::FrameMember { pairs, next_pair, .. } => (serde_json::to_value(pairs).unwrap(), *next_pair),
        _ => panic!(),
    };
    let (before, next) = stored(&app);
    let mut e = edit_dialog(&app, f, None).unwrap();
    assert_eq!(
        path(&e),
        [
            Sel::ExternalSketchCurve { sketch: base, id: l0.clone() },
            corner(600., 0., 0.),
            corner(600., 0., 400.),
            corner(0., 0., 400.),
            corner(600., 0., 400.),
        ]
    );
    let refs: Vec<Option<Value>> =
        std::iter::once(None).chain(before.as_array().unwrap().iter().flat_map(|p| [Some(p["a"].clone()), Some(p["b"].clone())])).collect();
    assert_eq!(member(&mut e).refs, refs);
    let c = apply_commands(&app, &e).unwrap();
    assert_eq!(c[0].0, "timeline.redefine");
    assert_eq!(c[0].1["params"]["pairs"], before, "each pair goes back with its id and its references verbatim");
    run_all(&mut app, c);
    assert_eq!(stored(&app), (before, next));
}

/// Editing an end cap shows a face for each saved end. When one cannot be found, the faces start
/// empty and OK names the missing end instead of dropping it unasked; what is picked again is taken,
/// even fewer ends.
#[test]
fn an_end_cap_edit_refuses_to_drop_an_end_it_cannot_find() {
    let tube = json!({"kind": "rect_tube", "width": 40, "height": 40, "wall": 3, "outer_radius": 0});
    let mut app = app_with(&[
        ("sketch.create", json!({"plane": "XY", "name": "Post path"})),
        ("sketch.options", json!({"sketch_3d": true})),
        ("sketch.line3d", json!({"points": [[0, 0, 0], [0, 0, 300]]})),
        ("sketch.finish", json!({})),
        ("frame.member", json!({"sketch": "Post path", "body_name": "Post", "profile": tube})),
    ]);
    let end = |app: &SolveApp, z: f64| {
        let st = app.session.model.state();
        let b = st.body("Post").unwrap();
        let i = (0..b.body.face_count())
            .find(|i| b.body.planar_face(*i).is_some_and(|pl| pl.normal().z.abs() > 0.999 && (pl.origin.z - z).abs() < 1e-6))
            .unwrap();
        Sel::Face { body: "Post".into(), index: i, point: solidcore_record::Vec3::new(18.5, 0.0, z) }
    };
    let (top, bottom) = (end(&app, 300.0), end(&app, 0.0));
    let faces: Vec<Value> = [&top, &bottom]
        .iter()
        .map(|s| match s {
            Sel::Face { body, index, point } => json!({"body": body, "index": index, "face": point}),
            _ => Value::Null,
        })
        .collect();
    app.session.execute("frame.end_cap", &json!({"faces": faces, "thickness": 6})).unwrap();
    let kind = app.session.doc.find_feature("EndCap1").unwrap().kind.clone();
    let (crate::dialogs::Kind::EndCap(c), inputs, extra) = crate::dialogs_end_cap::for_feature(&app, &kind).unwrap() else { panic!() };
    assert!(c.unresolved.is_empty() && inputs[0].items.len() == 2, "{c:?}");
    assert!(crate::dialogs_end_cap::commands(&app, &c, &inputs, &extra).is_ok());
    // The second end could not be found: nothing is picked, and OK says which end is missing.
    let c = crate::dialogs_end_cap::EndCap { unresolved: vec![2], ..c };
    let mut inputs = inputs;
    inputs[0].items.clear();
    let e = crate::dialogs_end_cap::commands(&app, &c, &inputs, &extra).unwrap_err();
    assert!(e.contains('2'), "{e}");
    // Picked again, the remaining end alone is a deliberate choice, as are both.
    inputs[0].items = vec![top.clone()];
    assert!(crate::dialogs_end_cap::commands(&app, &c, &inputs, &extra).is_ok());
    inputs[0].items = vec![top, bottom];
    assert!(crate::dialogs_end_cap::commands(&app, &c, &inputs, &extra).is_ok());
}

/// A 300 mm post up Z, a 40 × 40 square tube named `Post`, and a `Ref` sketch line on XY at 30°.
fn post_commands() -> Vec<(&'static str, Value)> {
    let tube = json!({"kind": "rect_tube", "width": 40, "height": 40, "wall": 3, "outer_radius": 0});
    vec![
        ("sketch.create", json!({"plane": "XY", "name": "Post path"})),
        ("sketch.options", json!({"sketch_3d": true})),
        ("sketch.line3d", json!({"points": [[0, 0, 0], [0, 0, 300]]})),
        ("sketch.finish", json!({})),
        ("frame.member", json!({"sketch": "Post path", "body_name": "Post", "profile": tube})),
        ("sketch.create", json!({"plane": "XY", "name": "Ref"})),
        ("sketch.line", json!({"points": [[100, 0], [100.0 + 100.0 * 3f64.sqrt() / 2.0, 50]], "ids": ["ref"]})),
        ("sketch.finish", json!({})),
    ]
}

fn post_app() -> SolveApp {
    app_with(&post_commands())
}

fn post_harness() -> crate::scenario::Harness {
    let mut h = crate::scenario::Harness::new();
    for (id, p) in post_commands() {
        h.app.run(id, p).unwrap();
    }
    h
}

/// The post's outer side facing +X, as the view picks it, and as the command takes it.
fn post_side(app: &SolveApp) -> (Sel, Value) {
    let st = app.session.model.state();
    let b = st.body("Post").unwrap();
    let i = (0..b.body.face_count())
        .find(|i| b.body.planar_face(*i).is_some_and(|pl| pl.normal().x > 0.999 && (pl.origin.x - 20.0).abs() < 1e-6))
        .unwrap();
    let point = solidcore_record::Vec3::new(20.0, 0.0, 150.0);
    (Sel::Face { body: "Post".into(), index: i, point }, json!({"face": point, "body": "Post", "index": i}))
}

fn rotate(d: &mut Dialog) -> &mut crate::dialogs_rotate_member::RotateMember {
    match &mut d.kind {
        Kind::RotateMember(r) => r,
        k => panic!("{k:?}"),
    }
}

/// The dialog's picks: as each frame's rows see them.
fn sync(app: &SolveApp, d: &mut Dialog) {
    let mut inputs = d.inputs.clone();
    crate::dialogs_rotate_member::sync(app, rotate(d), &mut inputs);
    d.inputs = inputs;
}

/// The dial of an angle-section post: on the post's profile's box centre (off its line: the line
/// is on a corner), at mid-length, along the line.
#[test]
fn rotate_member_dial_sits_on_the_turn_pivot() {
    let angle = json!({"kind": "angle", "width": 60, "height": 40, "thickness": 5});
    let app = app_with(&[
        ("sketch.create", json!({"plane": "XY", "name": "Post path"})),
        ("sketch.options", json!({"sketch_3d": true})),
        ("sketch.line3d", json!({"points": [[0, 0, 0], [0, 0, 300]]})),
        ("sketch.finish", json!({})),
        ("frame.member", json!({"sketch": "Post path", "body_name": "Post", "profile": angle, "anchor": "bottom_left"})),
    ]);
    // The post runs up Z, so its box centre is the profile's box centre at mid-length.
    let centre = app.session.model.state().body("Post").unwrap().mesh().bounds().center();
    assert!((centre.z - 150.0).abs() < 1e-6 && centre.x.hypot(centre.y) > 1.0, "an angle's box centre is off its line: {centre:?}");
    let st = app.session.model.state();
    let b = st.body("Post").unwrap();
    let index = (0..b.body.face_count()).find(|i| b.body.planar_face(*i).is_some()).unwrap();
    let mut inputs = crate::dialogs_rotate_member::start(&app, "frame.rotate_member").unwrap().1;
    inputs[0].items = vec![Sel::Face { body: "Post".into(), index, point: centre }];
    let (at, along) = crate::dialogs_rotate_member::dial(&app, &inputs).expect("a dial on the picked post");
    assert!(at.dist(centre) < 1e-6, "dial centre {at:?}, the turn's pivot {centre:?}");
    assert!(along.cross(solidcore_record::Vec3::Z).len() < 1e-9 && along.z > 0.0, "dial axis {along:?}, the post's line +Z");
}

/// No dial on a member inside a component, moved off the origin: the command refuses those
/// members (not supported yet), so the dial would set an angle OK can't use.
#[test]
fn rotate_member_no_dial_on_a_member_in_a_component() {
    let angle = json!({"kind": "angle", "width": 60, "height": 40, "thickness": 5});
    let mut app = app_with(&[
        ("component.create", json!({"name": "Frame", "grounded": false})),
        ("sketch.create", json!({"plane": "XY", "name": "Post path"})),
        ("sketch.options", json!({"sketch_3d": true})),
        ("sketch.line3d", json!({"points": [[0, 0, 0], [0, 0, 300]]})),
        ("sketch.finish", json!({})),
        ("frame.member", json!({"sketch": "Post path", "body_name": "Post", "profile": angle, "anchor": "bottom_left"})),
    ]);
    let frame = app.session.doc.find_component("Frame").unwrap();
    let occurrence = app.session.doc.occurrence_of(frame).unwrap().id;
    app.run("occurrence.move", json!({"occurrence": occurrence, "axis": [1, 2, 3], "angle": "37 deg", "translate": [50, 20, 10]})).unwrap();
    let world = crate::open_meshing::session_world(&app.session);
    let body = world.bodies.iter().find(|b| b.name.contains("Post")).unwrap_or_else(|| panic!("{:?}", body_names(&app)));
    let index = (0..body.body.face_count()).find(|i| body.body.planar_face(*i).is_some()).unwrap();
    let mut inputs = crate::dialogs_rotate_member::start(&app, "frame.rotate_member").unwrap().1;
    inputs[0].items = vec![Sel::Face { body: body.name.clone(), index, point: body.mesh().bounds().center() }];
    assert!(crate::dialogs_rotate_member::dial(&app, &inputs).is_none());
}

fn turns(app: &SolveApp) -> Vec<String> {
    app.session
        .doc
        .features
        .iter()
        .filter_map(|f| match &f.kind {
            FeatureKind::FrameMemberTurn { angle, .. } => Some(angle.to_string()),
            _ => None,
        })
        .collect()
}

/// The picked member and the typed angle make one `frame.rotate_member`.
#[test]
fn rotate_member_dialog_emits_its_command() {
    let mut app = post_app();
    let (side, face) = post_side(&app);
    app.start("frame.rotate_member");
    let mut d = app.dialog.clone().unwrap();
    d.inputs[0].items = vec![side];
    rotate(&mut d).angle = "30 deg".into();
    let c = apply_commands(&app, &d).unwrap();
    assert_eq!(c, [("frame.rotate_member".to_string(), json!({"face": face, "angle": "30 deg"}))]);
    run_all(&mut app, c);
    assert_eq!(turns(&app), ["30 deg"]);
}

/// A Parallel To pick that names no one line (a curve id in two sketches) says so on OK, rather
/// than OK leaving it out.
#[test]
fn rotate_member_dialog_reports_an_unresolved_parallel_to() {
    let mut app = post_app();
    app.run("sketch.create", json!({"plane": "XY", "name": "Ref 2"})).unwrap();
    app.run("sketch.line", json!({"points": [[0, 100], [50, 150]], "ids": ["ref"]})).unwrap();
    app.run("sketch.finish", json!({})).unwrap();
    let (side, _) = post_side(&app);
    app.start("frame.rotate_member");
    let mut d = app.dialog.clone().unwrap();
    d.inputs[0].items = vec![side];
    d.inputs[1].items = vec![Sel::SketchCurve { id: "ref".into(), sketch: None }];
    let e = apply_commands(&app, &d).unwrap_err();
    assert!(e.contains("only one sketch"), "{e}");
}

/// Edit Feature opens on the step's member and angle, and runs `timeline.redefine` even with the
/// angle unchanged.
#[test]
fn rotate_member_edit_opens_with_its_member_and_angle() {
    let mut app = post_app();
    let (_, face) = post_side(&app);
    app.run("frame.rotate_member", json!({"face": face, "angle": "30 deg"})).unwrap();
    let f = app.session.doc.features.iter().find(|f| matches!(f.kind, FeatureKind::FrameMemberTurn { .. })).unwrap().id;
    let mut e = edit_dialog(&app, f, None).unwrap();
    assert!(matches!(e.inputs[0].items.as_slice(), [Sel::Face { body, .. }] if body == "Post"), "{:?}", e.inputs[0].items);
    assert_eq!(rotate(&mut e).angle, "30 deg");
    sync(&app, &mut e);
    assert_eq!(rotate(&mut e).angle, "30 deg", "the step's own angle stays");
    assert_eq!(apply_commands(&app, &e).unwrap()[0].0, "timeline.redefine", "an edit back to the current turn still runs");
    rotate(&mut e).angle = "45 deg".into();
    let c = apply_commands(&app, &e).unwrap();
    run_all(&mut app, c);
    assert_eq!(turns(&app), ["45 deg"]);
}

/// OK on a new step at the member's current turn closes without adding one.
#[test]
fn rotate_member_dialog_same_angle_runs_nothing() {
    let mut h = post_harness();
    let (side, face) = post_side(&h.app);
    h.app.start("frame.rotate_member");
    let mut d = h.app.dialog.clone().unwrap();
    d.inputs[0].items = vec![side.clone()];
    sync(&h.app, &mut d);
    assert_eq!(rotate(&mut d).angle, "0 deg", "a member never turned shows 0");
    assert_eq!(apply_commands(&h.app, &d).unwrap(), []);
    h.app.run("frame.rotate_member", json!({"face": face, "angle": "30 deg"})).unwrap();
    h.app.start("frame.rotate_member");
    let mut d = h.app.dialog.clone().unwrap();
    d.inputs[0].items = vec![side];
    sync(&h.app, &mut d);
    assert_eq!(rotate(&mut d).angle, "30 deg", "the member's current turn");
    assert_eq!(apply_commands(&h.app, &d).unwrap(), []);
    rotate(&mut d).angle = "45 deg".into();
    assert_eq!(apply_commands(&h.app, &d).unwrap().len(), 1);
    // OK at the current turn closes the dialog and adds nothing.
    rotate(&mut d).angle = "30 deg".into();
    h.app.dialog = Some(d);
    h.frames(2);
    h.step(&json!({"key": "Enter"})).unwrap();
    h.settle();
    assert!(h.app.dialog.is_none());
    assert_eq!(turns(&h.app), ["30 deg"]);
}

/// A parameter at the member's current turn still adds a step: OK on `tilt` (0 deg) on a member
/// never turned adds one, and changing `tilt` turns the member.
#[test]
fn rotate_member_dialog_adds_a_parameter_angle_at_the_current_turn() {
    let mut app = post_app();
    app.run("parameters.add", json!({"name": "tilt", "expression": "0 deg"})).unwrap();
    let (side, _) = post_side(&app);
    app.start("frame.rotate_member");
    let mut d = app.dialog.clone().unwrap();
    d.inputs[0].items = vec![side];
    sync(&app, &mut d);
    rotate(&mut d).angle = "tilt".into();
    let c = apply_commands(&app, &d).unwrap();
    assert_eq!(c.len(), 1, "a step is added: {c:?}");
    run_all(&mut app, c);
    assert_eq!(turns(&app), ["tilt"]);
    let x = |app: &SolveApp| app.session.model.state().body("Post").unwrap().mesh().bounds().max.x;
    assert!((x(&app) - 20.0).abs() < 1e-6, "{}", x(&app));
    app.run("parameters.change", json!({"name": "tilt", "expression": "30 deg"})).unwrap();
    // Turned 30 deg, the 40 x 40 section spans 20 (cos 30 deg + sin 30 deg) each side.
    let half = 20.0 * (30f64.to_radians().cos() + 30f64.to_radians().sin());
    assert!((x(&app) - half).abs() < 1e-6, "{} vs {half}", x(&app));
}

/// The post's design with a second post, `Other`, 100 mm along -X.
fn two_posts_app() -> SolveApp {
    let tube = json!({"kind": "rect_tube", "width": 40, "height": 40, "wall": 3, "outer_radius": 0});
    let mut cmds = post_commands();
    cmds.extend([
        ("sketch.create", json!({"plane": "XY", "name": "Other path"})),
        ("sketch.options", json!({"sketch_3d": true})),
        ("sketch.line3d", json!({"points": [[-100, 0, 0], [-100, 0, 300]]})),
        ("sketch.finish", json!({})),
        ("frame.member", json!({"sketch": "Other path", "body_name": "Other", "profile": tube})),
    ]);
    app_with(&cmds)
}

/// Edit Feature opens with the member fixed: a click on another member leaves the member, the
/// Angle and the pick as they were.
#[test]
fn rotate_member_edit_keeps_its_member() {
    let mut app = two_posts_app();
    let (_, face) = post_side(&app);
    app.run("frame.rotate_member", json!({"face": face, "angle": "30 deg"})).unwrap();
    let f = app.session.doc.features.iter().find(|f| matches!(f.kind, FeatureKind::FrameMemberTurn { .. })).unwrap().id;
    let mut e = edit_dialog(&app, f, None).unwrap();
    let member = rotate(&mut e).member.clone();
    let picked = e.inputs[0].items.clone();
    let st = app.session.model.state();
    let other = st.body("Other").unwrap();
    let i = (0..other.body.face_count())
        .find(|i| other.body.planar_face(*i).is_some_and(|pl| pl.normal().x > 0.999 && (pl.origin.x + 80.0).abs() < 1e-6))
        .unwrap();
    e.inputs[0].items = vec![Sel::Face { body: "Other".into(), index: i, point: solidcore_record::Vec3::new(-80.0, 0.0, 150.0) }];
    sync(&app, &mut e);
    assert_eq!(rotate(&mut e).member, member, "the step's member");
    assert_eq!(rotate(&mut e).angle, "30 deg");
    assert_eq!(e.inputs[0].items, picked, "the pick goes back to the step's member");
    // Clearing the pick keeps the step's member and the Angle as typed.
    rotate(&mut e).angle = "45 deg".into();
    e.inputs[0].items.clear();
    sync(&app, &mut e);
    assert_eq!(rotate(&mut e).member, member, "a cleared pick keeps the step's member");
    assert_eq!(e.inputs[0].items, picked, "and puts its pick back");
    assert_eq!(rotate(&mut e).angle, "45 deg");
}

/// Editing a top-level step while another component is active: Parallel To still works the angle
/// out, in the step's own component as `timeline.redefine` runs it.
#[test]
fn rotate_member_edit_works_parallel_to_out_with_another_component_active() {
    let mut app = post_app();
    let (_, face) = post_side(&app);
    app.run("frame.rotate_member", json!({"face": face, "angle": "30 deg"})).unwrap();
    let f = app.session.doc.features.iter().find(|f| matches!(f.kind, FeatureKind::FrameMemberTurn { .. })).unwrap().id;
    app.run("component.create", json!({"name": "Other"})).unwrap();
    assert_ne!(app.session.active_component, 0, "another component is active");
    let sketch = sketch_id(&app, "Ref");
    let mut e = edit_dialog(&app, f, None).unwrap();
    e.inputs[1].items = vec![Sel::SketchCurve { id: "ref".into(), sketch: Some(sketch) }];
    sync(&app, &mut e);
    // The step is left out, so the side facing +X is square to the 30 deg line at -60 deg.
    assert_eq!(rotate(&mut e).angle, "-60 deg");
}

/// Picking Parallel To works the angle out (the side facing +X turned square to the 30° line:
/// −60°, the nearer of −60° and 120°); the Angle stays editable, and an edited one wins.
#[test]
fn rotate_member_parallel_to_fills_the_angle() {
    let mut app = post_app();
    let (side, face) = post_side(&app);
    let sketch = sketch_id(&app, "Ref");
    app.start("frame.rotate_member");
    let mut d = app.dialog.clone().unwrap();
    d.inputs[0].items = vec![side];
    d.inputs[1].items = vec![Sel::SketchCurve { id: "ref".into(), sketch: Some(sketch) }];
    sync(&app, &mut d);
    assert_eq!(rotate(&mut d).angle, "-60 deg");
    let c = apply_commands(&app, &d).unwrap();
    assert_eq!(c[0].1, json!({"face": face, "angle": "-60 deg", "parallel_to": {"sketch": sketch, "curve": "ref"}}));
    rotate(&mut d).angle = "-50 deg".into();
    let c = apply_commands(&app, &d).unwrap();
    assert_eq!(c[0].1, json!({"face": face, "angle": "-50 deg"}), "the typed angle wins");
    run_all(&mut app, c);
    assert_eq!(turns(&app), ["-50 deg"]);
}

/// The dial sits on the member once it is picked; dragging it a quarter turn sets the Angle, and
/// the camera stays put.
#[test]
fn rotate_member_dial_sets_the_angle() {
    let mut h = post_harness();
    h.app.preferences.nav = "openscad".into();
    h.step(&json!({"call": "ui.view", "params": {"view": "top"}})).unwrap();
    h.app.start("frame.rotate_member");
    h.frames(3);
    assert!(crate::scenario::handle_at("rotate_member").is_none(), "no dial before a member is picked");
    let (side, _) = post_side(&h.app);
    h.app.dialog.as_mut().unwrap().inputs[0].items = vec![side];
    h.frames(5);
    let camera = h.app.cam;
    let start = h.at(&json!({"handle": "rotate_member"})).unwrap();
    let centre = h.at(&json!({"world": [0, 0, 150]})).unwrap();
    // A quarter turn clockwise in the top view is -90 degrees about the post's line (+Z).
    let end = [centre[0] - (start[1] - centre[1]), centre[1] + (start[0] - centre[0])];
    h.step(&json!({"drag": [start, end], "steps": 12})).unwrap();
    assert_eq!(rotate(h.app.dialog.as_mut().unwrap()).angle, "-90 deg");
    assert_eq!(h.app.cam, camera, "the dial owns the drag");
    h.step(&json!({"key": "Enter"})).unwrap();
    h.settle();
    assert!(h.app.dialog.is_none());
    assert_eq!(turns(&h.app), ["-90 deg"]);
}

fn corner_type(name: &str) -> usize {
    CORNER_TYPES.iter().position(|c| c.0 == name).unwrap()
}

// ---------------------------------------------------------------------------------------------
// Ghost Frame Members (#15705)

fn tube() -> Value {
    json!({"kind": "rect_tube", "width": 40, "height": 40, "wall": 3})
}

/// The box with a rail over its top: a 40 mm tube `Rail` on the 3D sketch line from
/// (100, 200, 500) to (500, 200, 500), and an orthographic camera straight down over the middle
/// (0.75 px a millimetre in a 960 × 600 view).
fn railed_box() -> SolveApp {
    let mut app = box_app();
    for (id, p) in [
        ("sketch.create", json!({"plane": "XY", "name": "Rail path"})),
        ("sketch.options", json!({"sketch_3d": true})),
        ("sketch.line3d", json!({"points": [[100, 200, 500], [500, 200, 500]]})),
        ("sketch.finish", json!({})),
        ("frame.member", json!({"sketch": "Rail path", "body_name": "Rail", "profile": tube()})),
    ] {
        app.run(id, p).unwrap();
    }
    app.cam = solidcraft_viewport::Camera {
        target: Vec3::new(300.0, 200.0, 450.0),
        yaw: 0.0,
        pitch: std::f64::consts::FRAC_PI_2,
        roll: 0.0,
        distance: 1000.0,
        fov: 0.0,
        up: Default::default(),
    };
    app
}

/// The 960 × 600 model view `view.pick` answers for by default, as the mouse picker sees it.
fn model_proj(app: &SolveApp) -> crate::viewport::Proj {
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(960.0, 600.0));
    crate::viewport::Proj { cam: app.cam, vp: app.cam.view_proj(960.0 / 600.0, crate::viewport::scene_radius(app)), rect, bounds: None }
}

fn model_body(st: &solidcraft_engine::doc::ModelState, name: &str) -> solidcraft_engine::doc::ModelBody {
    st.bodies.iter().find(|b| b.name == name).unwrap().clone()
}

/// The colour the model scene paints the body in (its first face's): see-through below 255.
fn drawn(app: &SolveApp, body: &str) -> [u8; 4] {
    crate::viewport::build_scene(app);
    let arena = app.viewport.palette.lock().unwrap();
    arena.slots(body).and_then(|s| arena.get(s.face(0))).unwrap()
}

/// The first face the mouse picker finds under the centre of the view.
fn face_under_centre(app: &SolveApp) -> String {
    let hits = crate::viewport::pick(app, &model_proj(app), egui::pos2(480.0, 300.0));
    hits.iter()
        .find_map(|h| match h {
            crate::viewport::Hit::Face { body, .. } => Some(body.clone()),
            _ => None,
        })
        .unwrap()
}

fn frame_dialog(app: &mut SolveApp) -> Dialog {
    app.start("frame.member");
    app.dialog.clone().unwrap()
}

/// The Frame Member dialog with Ghost Frame Members ticked.
fn ghosted_frame_dialog(app: &mut SolveApp) -> Dialog {
    app.start("frame.member");
    member(app.dialog.as_mut().unwrap()).ghost = true;
    app.dialog.clone().unwrap()
}

#[test]
fn ghosting_is_off_when_the_dialog_opens() {
    let mut app = railed_box();
    let st = crate::open_meshing::shown_world(&app);
    let (rail, boxed) = (model_body(&st, "Rail"), model_body(&st, "Box"));
    assert!(!crate::viewport::frame_ghosted(&app, &rail), "no dialog: nothing is ghosted");
    let mut d = frame_dialog(&mut app);
    assert!(!member(&mut d).ghost);
    assert!(!crate::viewport::frame_ghosted(&app, &rail), "unticked: nothing is ghosted");
    member(app.dialog.as_mut().unwrap()).ghost = true;
    assert!(crate::viewport::frame_ghosted(&app, &rail));
    assert!(!crate::viewport::frame_ghosted(&app, &boxed), "only frame members are");
    // Not remembered: unticked again whenever the dialog opens, Edit Feature's too.
    crate::dialogs::cancel(&mut app);
    assert!(!member(&mut frame_dialog(&mut app)).ghost);
    crate::dialogs::cancel(&mut app);
    let f = app.session.doc.features.last().unwrap().id;
    assert!(!member(&mut edit_dialog(&app, f, None).unwrap()).ghost);
}

#[test]
fn a_click_on_a_ghosted_members_face_goes_through_to_what_is_behind() {
    let mut app = railed_box();
    ghosted_frame_dialog(&mut app);
    assert_eq!(face_under_centre(&app), "Box", "the mouse picks the box's top through the rail");
    let r = crate::view_pick::pick_command(&mut app, &json!({"frame": "model", "pixels": [[480, 300]]})).unwrap();
    let hit = &r["hits"][0];
    assert_eq!(hit["body"], "Box", "view.pick agrees: {hit}");
    assert!((hit["point"][2].as_f64().unwrap() - 400.0).abs() < 1e-6, "{hit}");
}

#[test]
fn a_ghosted_members_vertex_and_centreline_end_can_be_picked() {
    let mut app = railed_box();
    ghosted_frame_dialog(&mut app);
    let proj = model_proj(&app);
    let at = |p: Vec3| proj.to_screen(p).unwrap();
    // A corner of the tube's start (rounded: its vertices sit where the sides meet the arcs).
    let corner = Vec3::new(100.0, 180.0, 514.0);
    let hits = crate::viewport::pick(&app, &proj, at(corner));
    assert!(
        hits.iter().any(|h| matches!(h, crate::viewport::Hit::Vertex { body, point } if body == "Rail" && point.dist(corner) < 1e-6)),
        "{hits:?}"
    );
    let end = Vec3::new(500.0, 200.0, 500.0);
    let hits = crate::viewport::pick(&app, &proj, at(end));
    let rail = app.session.doc.features.last().unwrap().id;
    let path_sketch = sketch_id(&app, "Rail path");
    let wire = app.session.doc.sketch(path_sketch).unwrap().wires[0].id.clone();
    assert!(
        hits.iter().any(|h| matches!(h, crate::viewport::Hit::MemberEnd { feature, line, end: 1, point } if *feature == rail && *line == wire && point.dist(end) < 1e-6)),
        "{hits:?}"
    );
    // The path takes it as the point that end is built on: the 3D line's last node.
    let (_, sel) = crate::viewport::candidate(&app, &hits).unwrap();
    assert_eq!(sel, Sel::SketchPoint { id: format!("{path_sketch}:{wire}:1") });
    // A hidden member offers no end: the marker would take a click meant for what is shown.
    app.session.visibility.hidden_bodies.push("Rail".into());
    assert!(super::member_ends(&app).is_empty());
    assert!(!crate::viewport::pick(&app, &proj, at(end)).iter().any(|h| matches!(h, crate::viewport::Hit::MemberEnd { .. })));
}

#[test]
fn a_copys_hidden_crossing_tube_offers_no_end() {
    // An Auto plus, v (200, −100)…(200, 100) drawn before h (0, 0)…(400, 0): its members are v's two pieces, then h.
    // Named by position as a copy is, h's body is the third, not the second (a piece of v).
    let mut app = box_app();
    for (id, p) in [
        ("sketch.create", json!({"plane": "XY", "name": "Plus"})),
        ("sketch.line", json!({"points": [[200, -100], [200, 100]], "ids": ["v"]})),
        ("sketch.line", json!({"points": [[0, 0], [400, 0]], "ids": ["h"]})),
        ("sketch.finish", json!({})),
        ("frame.member", json!({"sketch": "Plus", "profile": tube(), "corner": "auto", "names": {"h": "H", "v": "V"}})),
    ] {
        app.run(id, p).unwrap();
    }
    let f = app.session.doc.features.last().unwrap().id;
    app.session.doc_mut().feature_mut(f).unwrap().body_names = ["V", "V · H", "H"].map(String::from).to_vec();
    app.session.refresh();
    ghosted_frame_dialog(&mut app);
    app.session.visibility.hidden_bodies.push("H".into());
    let lines: Vec<String> = super::member_ends(&app)
        .into_iter()
        .filter_map(|h| match h {
            crate::viewport::Hit::MemberEnd { feature, line, .. } if feature == f => Some(line),
            _ => None,
        })
        .collect();
    assert_eq!(lines, ["v", "v"], "h's body is hidden, v's shown");
}

#[test]
fn picking_a_centreline_end_stores_the_point_it_is_built_on() {
    let mut app = box_app();
    let base = sketch_id(&app, "Base");
    let l0 = app.session.doc.sketch(base).unwrap().curves[0].clone();
    let upright = json!({"a": vertex(600., 0., 0.), "b": vertex(600., 0., 400.)});
    app.run("frame.member", json!({"sketch": "Base", "lines": [l0.id], "profile": tube(), "pairs": [upright]})).unwrap();
    let f = app.session.doc.features.last().unwrap().id;
    let FeatureKind::FrameMember { pairs, .. } = &app.session.doc.feature(f).unwrap().kind else { panic!() };
    let pair_a = serde_json::to_value(&pairs[0].a).unwrap();
    let solidcraft_engine::sketch::CurveKind::Line { a, .. } = l0.kind else { panic!() };
    let start = app.session.doc.sketch(base).unwrap().points[a].id.clone();
    let mut d = ghosted_frame_dialog(&mut app);
    let ends = super::member_ends(&app);
    let end_of = |line: &str, end: usize| {
        ends.iter().find(|h| matches!(h, crate::viewport::Hit::MemberEnd { line: l, end: e, .. } if l == line && *e == end)).unwrap().clone()
    };
    for hit in [end_of(&l0.id, 0), end_of("pair1", 0)] {
        let (_, sel) = crate::viewport::candidate(&app, std::slice::from_ref(&hit)).unwrap();
        pick_hit(&app, &mut d, &hit, sel);
    }
    assert_eq!(path(&d)[0], Sel::SketchPoint { id: format!("{base}:{start}") });
    let refs = member(&mut d).refs.clone();
    assert_eq!(refs, [Some(json!({"sketch": base, "point": start})), Some(pair_a.clone())], "the line's end point, and a copy of the pair's");
    assert_eq!(apply_commands(&app, &d).unwrap()[0].1["pairs"][0]["b"], pair_a);
}

#[test]
fn this_frame_members_tubes_are_tinted_apart_from_other_frame_members() {
    let mut app = railed_box();
    let mut d = ghosted_frame_dialog(&mut app);
    pick_all(&app, &mut d, &[corner(0., 0., 400.), corner(600., 0., 400.)]);
    app.dialog = Some(d.clone());
    let colors = crate::preview::colors(&app);
    let built = crate::preview::compute(&app.session, &apply_commands(&app, &d).unwrap(), colors).unwrap();
    assert!(colors.glass_new);
    assert!(built.scene.tris.is_empty() && !built.scene.glass.is_empty(), "this feature's new tube is drawn see-through");
    let own = colors.new_body;
    let other = drawn(&app, "Rail");
    assert!(own[3] < 255 && other[3] < 255, "both see-through: {own:?} {other:?}");
    let body = solidcraft_theme::Tokens::get().body.to_srgba_unmultiplied();
    assert_eq!(other[..3], body[..3], "other frame members are grey");
    assert_ne!(own[..3], other[..3], "this one's are tinted");
}

/// With a component placed away from its frame the preview is of the placed model; this feature's
/// new tubes are still drawn see-through in the ghost colour there.
#[test]
fn a_placed_models_preview_keeps_the_ghost_colour() {
    let mut app = box_app();
    let c = app.run("component.create", json!({"name": "Away", "grounded": false})).unwrap()["component"].as_u64().unwrap();
    let o = app.session.doc.occurrence_of(c).unwrap().id;
    app.run("occurrence.move", json!({"occurrence": o, "translate": [0, 0, 1000]})).unwrap();
    app.run("component.activate", json!({"component": 0})).unwrap();
    assert!(solidcraft_engine::frames::placed(&app.session));
    let mut d = ghosted_frame_dialog(&mut app);
    pick_all(&app, &mut d, &[corner(0., 0., 400.), corner(600., 0., 400.)]);
    app.dialog = Some(d.clone());
    let colors = crate::preview::colors(&app);
    let built = crate::preview::compute(&app.session, &apply_commands(&app, &d).unwrap(), colors).unwrap();
    assert!(built.scene.tris.is_empty() && !built.scene.glass.is_empty(), "the new tube is drawn see-through");
    let drawn: std::collections::BTreeSet<[u8; 4]> = built
        .scene
        .glass
        .chunks(crate::gpu::TRI_SIZE)
        .filter_map(|v| v.get(24..28).and_then(|b| b.try_into().ok()).map(u32::from_le_bytes))
        .filter_map(|i| built.scene.palette.get(crate::gpu::PaletteIndex(i)))
        .collect();
    assert_eq!(drawn.into_iter().collect::<Vec<_>>(), [colors.new_body], "in the ghost colour, its alpha {}", colors.new_body[3]);
}

#[test]
fn with_ghosting_off_members_draw_and_pick_as_today() {
    let mut app = railed_box();
    let opaque = drawn(&app, "Rail");
    assert_eq!(opaque[3], 255);
    assert_eq!(face_under_centre(&app), "Rail");
    let mut d = frame_dialog(&mut app);
    member(&mut d).ghost = false;
    app.dialog = Some(d);
    assert_eq!(drawn(&app, "Rail"), opaque);
    assert_eq!(face_under_centre(&app), "Rail");
    let r = crate::view_pick::pick_command(&mut app, &json!({"frame": "model", "pixels": [[480, 300]]})).unwrap();
    assert_eq!(r["hits"][0]["body"], "Rail");
    assert!(super::member_ends(&app).is_empty(), "no centreline ends to pick");
    let colors = crate::preview::colors(&app);
    assert!(colors.new_body[3] == 255 && !colors.glass_new, "the preview's new tubes are solid");
}

#[test]
fn end_caps_are_ghosted_with_members() {
    let mut app = app_with(&[
        ("sketch.create", json!({"plane": "XY", "name": "Post path"})),
        ("sketch.options", json!({"sketch_3d": true})),
        ("sketch.line3d", json!({"points": [[0, 0, 0], [0, 0, 300]]})),
        ("sketch.finish", json!({})),
        ("frame.member", json!({"sketch": "Post path", "body_name": "Post", "profile": tube()})),
    ]);
    let st = app.session.model.state();
    let b = st.body("Post").unwrap();
    let top = (0..b.body.face_count())
        .find(|i| b.body.planar_face(*i).is_some_and(|pl| pl.normal().z > 0.999 && (pl.origin.z - 300.0).abs() < 1e-6))
        .unwrap();
    app.run("frame.end_cap", json!({"faces": [{"body": "Post", "index": top, "face": [18.5, 0.0, 300.0]}], "thickness": 6})).unwrap();
    let st = crate::open_meshing::shown_world(&app);
    let cap = st.bodies.iter().find(|b| b.name != "Post").unwrap().name.clone();
    assert_eq!(drawn(&app, &cap)[3], 255);
    ghosted_frame_dialog(&mut app);
    assert!(crate::viewport::frame_ghosted(&app, &model_body(&st, &cap)));
    assert!(drawn(&app, &cap)[3] < 255 && drawn(&app, "Post")[3] < 255);
}

// ---------------------------------------------------------------------------------------------
// Remove Tube in the dialog (#15705)

/// The box seen from straight above, as `railed_box` sees it.
fn from_above(app: &mut SolveApp) {
    app.cam = solidcraft_viewport::Camera {
        target: Vec3::new(300.0, 200.0, 450.0),
        yaw: 0.0,
        pitch: std::f64::consts::FRAC_PI_2,
        roll: 0.0,
        distance: 1000.0,
        fov: 0.0,
        up: Default::default(),
    };
}

/// The open dialog's live preview, computed to the end.
fn settle_preview(app: &mut SolveApp) {
    let ctx = egui::Context::default();
    for _ in 0..4000 {
        crate::preview::update(app, &ctx);
        if !app.preview.busy {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    panic!("the preview did not finish");
}

/// The Frame Member dialog open with `picks` in its path, its preview computed.
fn dialog_with(app: &mut SolveApp, picks: &[Sel]) {
    let mut d = frame_dialog(app);
    pick_all(app, &mut d, picks);
    app.dialog = Some(d);
    settle_preview(app);
}

fn open(app: &mut SolveApp) -> &mut Dialog {
    app.dialog.as_mut().unwrap()
}

fn picked_tubes(app: &mut SolveApp) -> Vec<String> {
    member(open(app)).picked_tubes.clone()
}

fn centreline(line: &str) -> crate::viewport::Hit {
    crate::viewport::Hit::Centreline { line: line.into() }
}

/// A diagonal over the box's top, seen from above: its ends on screen.
fn diagonal(app: &mut SolveApp) -> (egui::Pos2, egui::Pos2) {
    from_above(app);
    dialog_with(app, &[corner(0., 0., 400.), corner(600., 400., 400.)]);
    let proj = model_proj(app);
    (proj.to_screen(Vec3::new(0., 0., 400.)).unwrap(), proj.to_screen(Vec3::new(600., 400., 400.)).unwrap())
}

fn menu_labels(app: &SolveApp) -> Vec<(String, String)> {
    crate::context_menu::items(app, &crate::context_menu::Target::FrameTubes).into_iter().map(|i| (i.label, i.icon.to_string())).collect()
}

#[test]
fn a_centreline_is_hovered_within_8_px_and_clicked_to_select() {
    let mut app = box_app();
    let (a, b) = diagonal(&mut app);
    let lines = &app.preview.centrelines;
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!((lines[0].id.as_str(), lines[0].pair), ("pair1", Some(0)));
    let proj = model_proj(&app);
    let across = (b - a).normalized().rot90();
    let middle = a + (b - a) * 0.5;
    let hover = |app: &SolveApp, px: f32| super::tube_hover(app, &crate::viewport::pick(app, &proj, middle + across * px));
    assert_eq!(hover(&app, 6.0), Some(centreline("pair1")));
    assert_eq!(hover(&app, -6.0), Some(centreline("pair1")));
    assert_eq!(hover(&app, 10.0), None, "too far off");
    assert!(super::click_tube(&mut app, &centreline("pair1")));
    assert_eq!(picked_tubes(&mut app), ["pair1"]);
    assert!(super::click_tube(&mut app, &centreline("pair1")));
    assert!(picked_tubes(&mut app).is_empty(), "a second click takes it out of the selection");
    assert!(!super::click_tube(&mut app, &crate::viewport::Hit::Axis { name: "X".into() }), "only a centreline");
    // A right-click on a centreline selects it and opens its menu: one item, Remove Tube.
    app.viewport.hover = Some(centreline("pair1"));
    assert!(super::open_menu(&mut app, middle));
    assert_eq!(app.menu.target, crate::context_menu::Target::FrameTubes);
    assert_eq!(picked_tubes(&mut app), ["pair1"]);
    assert_eq!(menu_labels(&app), [("Remove Tube".to_string(), "delete".to_string())]);
    // With Right-click confirms on, the right-click confirms the input as before: no menu.
    app.preferences.right_click_confirms = true;
    assert!(!super::open_menu(&mut app, middle));
}

/// A click on a line member's sketch line picks the line as before (it comes out of the path):
/// the centreline over it takes a click only where nothing else would be picked.
#[test]
fn a_line_under_a_line_members_centreline_is_picked_not_the_tube() {
    let mut app = frame_app();
    let frame = sketch_id(&app, "Frame");
    let id = frame_lines(&app)[0].clone();
    from_above(&mut app);
    dialog_with(&mut app, &[Sel::ExternalSketchCurve { sketch: frame, id: id.clone() }]);
    let proj = model_proj(&app);
    let c = app.preview.centrelines.first().cloned().unwrap();
    // A third of the way along, clear of the line's ends and its middle.
    let at = proj.to_screen(c.a + (c.b - c.a) * (1.0 / 3.0)).unwrap();
    let hits = crate::viewport::pick(&app, &proj, at);
    assert!(hits.contains(&centreline(&id)), "{hits:?}");
    assert_eq!(super::tube_hover(&app, &hits), None);
    assert!(matches!(crate::viewport::candidate(&app, &hits), Some((crate::viewport::Hit::SketchCurve { .. }, _))), "{hits:?}");
}

#[test]
fn a_point_under_the_cursor_wins_over_a_centreline() {
    let mut app = box_app();
    let (a, _) = diagonal(&mut app);
    let hits = crate::viewport::pick(&app, &model_proj(&app), a);
    assert!(hits.contains(&centreline("pair1")), "{hits:?}");
    assert!(hits.iter().any(|h| matches!(h, crate::viewport::Hit::Vertex { body, .. } if body == "Box")), "{hits:?}");
    assert_eq!(super::tube_hover(&app, &hits), None, "the corner is picked, not the tube");
}

#[test]
fn remove_tube_on_a_pair_takes_out_its_two_points_and_keeps_a_shared_one() {
    let mut app = box_app();
    let (p0, p1, p2) = (corner(0., 0., 0.), corner(0., 0., 400.), corner(600., 0., 400.));
    dialog_with(&mut app, &[p0, p1.clone(), p1.clone(), p2.clone()]);
    assert_eq!(app.preview.centrelines.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["pair1", "pair2"]);
    let refs = member(open(&mut app)).refs.clone();
    super::remove_tubes(&mut app, &["pair1".to_string()]);
    let d = app.dialog.clone().unwrap();
    assert_eq!(path(&d), [p1, p2], "only this pair's occurrence of the shared point goes");
    assert_eq!(member(open(&mut app)).refs, refs[2..], "the others keep their references");
    assert_eq!(apply_commands(&app, &d).unwrap()[0].1["pairs"], json!([{"a": vertex(0., 0., 400.), "b": vertex(600., 0., 400.)}]));
}

#[test]
fn remove_tube_on_a_line_member_unpicks_its_line() {
    let mut app = box_app();
    let base = sketch_id(&app, "Base");
    let ids: Vec<String> = app.session.doc.sketch(base).unwrap().curves.iter().map(|c| c.id.clone()).collect();
    let line = |id: &String| Sel::ExternalSketchCurve { sketch: base, id: id.clone() };
    dialog_with(&mut app, &[line(&ids[0]), line(&ids[1]), corner(0., 0., 0.), corner(0., 0., 400.)]);
    assert_eq!(app.preview.centrelines.iter().map(|c| c.id.clone()).collect::<Vec<_>>(), [ids[0].clone(), ids[1].clone(), "pair1".into()]);
    super::remove_tubes(&mut app, std::slice::from_ref(&ids[0]));
    let d = app.dialog.clone().unwrap();
    assert_eq!(path(&d), [line(&ids[1]), corner(0., 0., 0.), corner(0., 0., 400.)]);
    let p = &apply_commands(&app, &d).unwrap()[0].1;
    assert_eq!((&p["lines"], p["pairs"].as_array().map(Vec::len)), (&json!([ids[1]]), Some(1)));
    // On every line of the sketch (All Lines): the others are listed from then on.
    crate::dialogs::cancel(&mut app);
    app.run("frame.member", json!({"sketch": "Base", "profile": tube()})).unwrap();
    let f = app.session.doc.features.last().unwrap().id;
    app.edit_feature(f);
    settle_preview(&mut app);
    assert!(member(open(&mut app)).all);
    super::remove_tubes(&mut app, std::slice::from_ref(&ids[2]));
    assert!(!member(open(&mut app)).all);
    let d = app.dialog.clone().unwrap();
    let rest: Vec<&String> = ids.iter().filter(|id| **id != ids[2]).collect();
    assert_eq!(apply_commands(&app, &d).unwrap()[0].1["params"]["lines"], json!(rest));
}

#[test]
fn delete_removes_the_selected_tubes() {
    let mut app = box_app();
    let selection = app.session.selection.clone();
    let features = app.session.doc.features.len();
    dialog_with(&mut app, &[corner(0., 0., 0.), corner(0., 0., 400.), corner(600., 0., 0.), corner(600., 0., 400.)]);
    for id in ["pair1", "pair2"] {
        assert!(super::click_tube(&mut app, &centreline(id)));
    }
    assert_eq!(menu_labels(&app), [("Remove 2 Tubes".to_string(), "delete".to_string())]);
    crate::delete::delete_selection(&mut app);
    assert!(path(app.dialog.as_ref().unwrap()).is_empty(), "both pairs go");
    assert!(picked_tubes(&mut app).is_empty());
    assert_eq!((&app.session.selection, app.session.doc.features.len()), (&selection, features), "nothing else is deleted");
    // The menu's item does the same.
    let mut d = app.dialog.clone().unwrap();
    pick_all(&app, &mut d, &[corner(0., 0., 0.), corner(0., 0., 400.)]);
    app.dialog = Some(d);
    settle_preview(&mut app);
    assert!(super::click_tube(&mut app, &centreline("pair1")));
    let item = crate::context_menu::items(&app, &crate::context_menu::Target::FrameTubes).remove(0);
    crate::context_menu::run_item(&mut app, &item, egui::Pos2::ZERO);
    assert!(path(app.dialog.as_ref().unwrap()).is_empty());
}

#[test]
fn cancel_after_remove_tube_leaves_the_feature_unchanged() {
    let mut app = box_app();
    let pairs = json!([{"a": vertex(0., 0., 0.), "b": vertex(0., 0., 400.)}, {"a": vertex(0., 0., 400.), "b": vertex(600., 0., 400.)}]);
    app.run("frame.member", json!({"pairs": pairs, "profile": tube()})).unwrap();
    let f = app.session.doc.features.last().unwrap().id;
    let stored = |app: &SolveApp| serde_json::to_value(&app.session.doc.feature(f).unwrap().kind).unwrap();
    let (before, bodies) = (stored(&app), members(&app));
    app.edit_feature(f);
    settle_preview(&mut app);
    super::remove_tubes(&mut app, &["pair1".to_string()]);
    let d = app.dialog.clone().unwrap();
    let c = apply_commands(&app, &d).unwrap();
    assert_eq!(c[0].1["params"]["pairs"][0]["id"], "pair2", "the pair kept keeps its id: {}", c[0].1);
    assert_eq!(c[0].1["params"]["pairs"].as_array().map(Vec::len), Some(1));
    crate::dialogs::cancel(&mut app);
    assert!(app.dialog.is_none());
    assert_eq!((stored(&app), members(&app)), (before, bodies));
}

/// Remove Tube on every tube of an edited Frame Member: Cancel leaves it, OK deletes it (as
/// `frame.remove_member` does), in one undo step.
#[test]
fn ok_after_removing_every_tube_deletes_the_frame_member() {
    let mut app = box_app();
    let pairs = json!([{"a": vertex(0., 0., 0.), "b": vertex(0., 0., 400.)}, {"a": vertex(0., 0., 400.), "b": vertex(600., 0., 400.)}]);
    app.run("frame.member", json!({"pairs": pairs, "profile": tube()})).unwrap();
    let f = app.session.doc.features.last().unwrap().id;
    let (doc, bodies, undo) = (serde_json::to_value(&*app.session.doc).unwrap(), members(&app), app.session.undo.len());
    let empty = |app: &mut SolveApp| {
        app.edit_feature(f);
        settle_preview(app);
        super::remove_tubes(app, &["pair1".to_string(), "pair2".to_string()]);
        settle_preview(app);
        assert!(path(app.dialog.as_ref().unwrap()).is_empty());
        assert_eq!(app.preview.error, None);
    };
    empty(&mut app);
    crate::dialogs::cancel(&mut app);
    assert_eq!((serde_json::to_value(&*app.session.doc).unwrap(), members(&app)), (doc, bodies.clone()), "Cancel changes nothing");
    empty(&mut app);
    let d = app.dialog.clone().unwrap();
    crate::dialogs::run_dialog(&mut app, &d).unwrap();
    assert!(app.session.doc.feature(f).is_none(), "the Frame Member is deleted");
    assert!(members(&app).is_empty(), "{:?}", members(&app));
    assert_eq!(app.session.undo.len(), undo + 1, "one undo step");
    app.run("edit.undo", json!({})).unwrap();
    assert_eq!(members(&app), bodies);
}

/// The box with a circle drawn in sketch `Hoop`, and the circle as a pick: a line no member can
/// be laid on.
fn hoop_app() -> (SolveApp, Sel) {
    let mut app = box_app();
    for (id, p) in [
        ("sketch.create", json!({"plane": "XY", "name": "Hoop"})),
        ("sketch.circle.center", json!({"center": [0, 0], "radius": 50})),
        ("sketch.finish", json!({})),
    ] {
        app.run(id, p).unwrap();
    }
    let hoop = sketch_id(&app, "Hoop");
    let circle = Sel::ExternalSketchCurve { sketch: hoop, id: app.session.doc.sketch(hoop).unwrap().curves[0].id.clone() };
    (app, circle)
}

/// `picks` made in the open dialog, as clicks make them.
fn pick_in(app: &mut SolveApp, picks: &[Sel]) {
    let mut d = app.dialog.take().unwrap();
    pick_all(app, &mut d, picks);
    app.dialog = Some(d);
}

/// A line refused in place of the waiting point gives the point back as it was picked, with its
/// reference: once the live preview of the path it made is in and fails, so a pick never waits for
/// a build on the UI thread. A valid pick after it is kept.
#[test]
fn a_refused_line_gives_the_waiting_point_back_with_its_reference() {
    let (mut app, circle) = hoop_app();
    let d = dialog_on(&mut app, &[]);
    app.dialog = Some(d);
    pick_in(&mut app, &[corner(0., 0., 0.), corner(0., 0., 400.)]);
    settle_preview(&mut app);
    // A reference other than the one the corner would be found as again.
    let given = json!({"vertex": [600.0, 0.0, 0.001]});
    let mut d = app.dialog.take().unwrap();
    super::pick_as(&app, &mut d, corner(600., 0., 0.), Some(given.clone()));
    pick(&app, &mut d, circle.clone());
    assert!(!d.error.as_deref().is_some_and(super::refused_pick), "{:?}", d.error);
    assert_eq!(path(&d).last(), Some(&circle), "taken until its preview is in");
    app.dialog = Some(d);
    settle_preview(&mut app);
    let d = app.dialog.as_ref().unwrap();
    assert!(d.error.as_deref().is_some_and(super::refused_pick), "{:?}", d.error);
    assert_eq!(path(d), [corner(0., 0., 0.), corner(0., 0., 400.), corner(600., 0., 0.)]);
    settle_preview(&mut app);
    pick_in(&mut app, &[corner(600., 0., 400.)]);
    settle_preview(&mut app);
    let d = app.dialog.clone().unwrap();
    assert!(d.error.is_none() && app.preview.error.is_none(), "{:?} {:?}", d.error, app.preview.error);
    assert_eq!(path(&d).len(), 4, "the valid pick is kept");
    let c = apply_commands(&app, &d).unwrap();
    assert_eq!(c[0].1["pairs"][1]["a"], given, "{}", c[0].1);
}

/// A valid pick is kept when the frame fails for a change made after it (a wall thicker than the
/// tube), before its preview is in: that preview's error is not the pick's.
#[test]
fn a_change_after_a_valid_pick_does_not_take_it_back() {
    let mut app = box_app();
    let d = dialog_on(&mut app, &[]);
    app.dialog = Some(d);
    pick_in(&mut app, &[corner(0., 0., 0.), corner(0., 0., 400.)]);
    let m = member(open(&mut app));
    let kinds = frame::profile_kinds();
    let k = &kinds[m.kind];
    let wall = k.required.iter().chain(k.optional).position(|n| *n == "wall").unwrap();
    m.dims[m.kind][wall] = "30".into();
    settle_preview(&mut app);
    let d = app.dialog.as_ref().unwrap();
    assert!(app.preview.error.is_some(), "the frame fails");
    assert!(!d.error.as_deref().is_some_and(super::refused_pick), "{:?}", d.error);
    assert_eq!(path(d), [corner(0., 0., 0.), corner(0., 0., 400.)]);
}

/// A failing pick's preview takes back only that pick: a pick made after it, before that preview
/// is in, stays, though the path it made fails too.
#[test]
fn a_preview_of_an_older_path_takes_no_newer_pick_back() {
    let (mut app, circle) = hoop_app();
    let d = dialog_on(&mut app, &[]);
    app.dialog = Some(d);
    pick_in(&mut app, std::slice::from_ref(&circle));
    crate::preview::update(&mut app, &egui::Context::default());
    assert!(app.preview.busy, "the circle's path is previewed");
    pick_in(&mut app, &[corner(0., 0., 0.), corner(0., 0., 400.)]);
    settle_preview(&mut app);
    let d = app.dialog.as_ref().unwrap();
    assert!(app.preview.error.is_some());
    assert!(!d.error.as_deref().is_some_and(super::refused_pick), "{:?}", d.error);
    assert_eq!(path(d), [circle, corner(0., 0., 0.), corner(0., 0., 400.)]);
}

/// In a design with a component placed away from its frame the preview is of the placed model,
/// and it still gives the dialog the centrelines to pick for Remove Tube.
#[test]
fn a_design_with_a_placed_component_still_previews_centrelines() {
    let mut app = box_app();
    let c = app.run("component.create", json!({"name": "Away", "grounded": false})).unwrap()["component"].as_u64().unwrap();
    let o = app.session.doc.occurrence_of(c).unwrap().id;
    app.run("occurrence.move", json!({"occurrence": o, "translate": [0, 0, 1000]})).unwrap();
    app.run("component.activate", json!({"component": 0})).unwrap();
    assert!(solidcraft_engine::frames::placed(&app.session));
    dialog_with(&mut app, &[corner(0., 0., 0.), corner(0., 0., 400.)]);
    assert_eq!(app.preview.centrelines.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["pair1"]);
}

/// While OK computes, the path holds: Ctrl+click and Remove Tube take nothing out.
#[test]
fn the_path_holds_while_ok_computes() {
    let mut app = box_app();
    let picks = [corner(0., 0., 0.), corner(0., 0., 400.)];
    dialog_with(&mut app, &picks);
    let (_finish, done) = std::sync::mpsc::channel();
    app.applying = Some(crate::dialogs::Applying {
        done,
        ready: None,
        seen: true,
        started: 0.0,
        cmds: Vec::new(),
        progress: Default::default(),
        quality: Default::default(),
    });
    assert!(!super::take_out(&mut app, &picks[1]));
    super::remove_tubes(&mut app, &["pair1".to_string()]);
    assert_eq!(path(app.dialog.as_ref().unwrap()), picks);
}

/// Remove Tube while the preview is computing (of the same path: picks go when the path changes)
/// still takes the picked tubes out, from the last finished preview's centrelines.
#[test]
fn remove_tube_while_the_preview_is_busy_still_removes_the_picked_tubes() {
    let mut app = box_app();
    dialog_with(&mut app, &[corner(0., 0., 0.), corner(0., 0., 400.), corner(600., 0., 0.), corner(600., 0., 400.)]);
    assert!(super::click_tube(&mut app, &centreline("pair1")));
    app.preview.busy = true;
    assert!(super::remove_picked(&mut app));
    assert_eq!(path(app.dialog.as_ref().unwrap()), [corner(600., 0., 0.), corner(600., 0., 400.)], "pair1's two points go");
}

/// The view's pick cache knows the centrelines it offers: they go when the preview starts again
/// and come back when it finishes, and Ghost Frame Members changes what a click goes through.
#[test]
fn the_pick_cache_follows_the_previews_centrelines_and_ghosting() {
    let mut app = box_app();
    let (a, b) = diagonal(&mut app);
    let proj = model_proj(&app);
    let middle = a + (b - a) * 0.5;
    let tube_hit = |app: &mut SolveApp| crate::viewport::pick_cached(app, &proj, middle).contains(&centreline("pair1"));
    assert!(tube_hit(&mut app));
    app.preview.busy = true;
    assert!(!tube_hit(&mut app), "no centreline while the preview computes");
    app.preview.busy = false;
    assert!(tube_hit(&mut app));
    let mut app = railed_box();
    ghosted_frame_dialog(&mut app);
    let proj = model_proj(&app);
    let first_face = |app: &mut SolveApp| {
        crate::viewport::pick_cached(app, &proj, egui::pos2(480.0, 300.0)).into_iter().find_map(|h| match h {
            crate::viewport::Hit::Face { body, .. } => Some(body),
            _ => None,
        })
    };
    assert_eq!(first_face(&mut app).as_deref(), Some("Box"), "ghosted, the click goes through the rail");
    member(open(&mut app)).ghost = false;
    assert_eq!(first_face(&mut app).as_deref(), Some("Rail"));
}

// ---------------------------------------------------------------------------------------------
// The path's picked list (#15705)

/// The box, and the Frame Member dialog open on it in a harness with the given path picks made
/// as clicks in the view make them.
fn path_harness(picks: impl FnOnce(&SolveApp) -> Vec<Sel>) -> crate::scenario::Harness {
    let mut h = tall_harness();
    for (id, p) in [
        ("sketch.create", json!({"plane": "XY", "name": "Base"})),
        ("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [600, 400]})),
        ("sketch.finish", json!({})),
        ("solid.extrude", json!({"distance": 400, "name": "Box", "body_name": "Box"})),
    ] {
        h.app.run(id, p).unwrap();
    }
    h.app.start("frame.member");
    let mut d = h.app.dialog.take().unwrap();
    pick_all(&h.app, &mut d, &picks(&h.app));
    h.app.dialog = Some(d);
    h.frames(2);
    h
}

/// The Base sketch's first line as the view picks it.
fn base_line(app: &SolveApp) -> Sel {
    let base = sketch_id(app, "Base");
    Sel::ExternalSketchCurve { sketch: base, id: app.session.doc.sketch(base).unwrap().curves[0].id.clone() }
}

/// The number of rows in the Path list, opened.
fn path_rows(h: &mut crate::scenario::Harness) -> usize {
    if h.app.dialog.as_ref().unwrap().listed != Some(0) {
        h.step(&json!({"click": {"handle": "dialog:list_toggle:Path"}})).unwrap();
        h.frames(2);
    }
    (0..8).take_while(|i| crate::scenario::handle_at(&format!("dialog:list:Path:{i}:remove")).is_some()).count()
}

fn drawn_texts(h: &crate::scenario::Harness) -> Vec<String> {
    h.shapes.iter().filter_map(|s| if let egui::Shape::Text(t) = &s.shape { Some(t.galley.text().to_owned()) } else { None }).collect()
}

/// A line and a pair of points: two rows, the line's and the pair's, which reads as its tube; the
/// chip counts the two.
#[test]
fn a_point_pair_is_one_path_row() {
    let mut h = path_harness(|app| vec![base_line(app), corner(0., 0., 0.), corner(0., 0., 400.)]);
    assert_eq!(path(h.app.dialog.as_ref().unwrap()).len(), 3);
    assert!(drawn_texts(&h).iter().any(|t| t == "2 selected"), "the line and the tube");
    assert_eq!(path_rows(&mut h), 2, "the line's row and the pair's");
    let texts = drawn_texts(&h);
    let tube = texts.iter().find(|t| t.starts_with("Tube 1: ")).unwrap_or_else(|| panic!("a Tube 1 row: {texts:?}"));
    assert_eq!(tube.matches(" → ").count(), 1, "{tube}");
}

/// A lone first point makes no row: the dialog asks for the second, and the view still draws the
/// point (it stays in the path). The second point makes the pair's row.
#[test]
fn a_lone_first_point_is_not_a_row_and_the_prompt_asks_for_the_second() {
    let mut h = path_harness(|app| vec![base_line(app), corner(0., 0., 0.)]);
    let d = h.app.dialog.as_ref().unwrap();
    assert_eq!(d.error.as_deref(), Some("Pick the second point"));
    assert!(path(d).contains(&corner(0., 0., 0.)), "the waiting point is still picked, so drawn");
    assert!(drawn_texts(&h).iter().any(|t| t == "1 selected"), "the line alone");
    assert_eq!(path_rows(&mut h), 1, "the line's row only");
    assert!(!drawn_texts(&h).iter().any(|t| t.starts_with("Tube")));
    let mut d = h.app.dialog.take().unwrap();
    pick(&h.app, &mut d, corner(0., 0., 400.));
    h.app.dialog = Some(d);
    h.frames(2);
    assert_eq!(h.app.dialog.as_ref().unwrap().error, None);
    assert_eq!(path_rows(&mut h), 2);
}

/// A lone first point is no row yet: the Path chip stays the plain "Select" it is with nothing
/// picked, never a filled "0 selected" (the prompt asks for the second point).
#[test]
fn a_lone_first_point_leaves_the_path_chip_unfilled() {
    let empty = drawn_texts(&path_harness(|_| Vec::new()));
    let h = path_harness(|_| vec![corner(0., 0., 0.)]);
    assert_eq!(h.app.dialog.as_ref().unwrap().error.as_deref(), Some("Pick the second point"));
    let texts = drawn_texts(&h);
    assert!(!texts.iter().any(|t| t.ends_with("selected")), "{texts:?}");
    let selects = |t: &[String]| t.iter().filter(|t| *t == "Select").count();
    assert_eq!(selects(&texts), selects(&empty), "{texts:?} vs {empty:?}");
}

/// p0, p1, p1, p2 (two pairs sharing p1): two rows. Removing the second pair's row takes out both
/// its points, and the first pair keeps its points, references and id.
#[test]
fn removing_a_pair_row_removes_both_points() {
    let mut h = crate::scenario::Harness::new();
    let rail = json!({"a": vertex(0., 0., 0.), "b": vertex(0., 0., 400.)});
    let top = json!({"a": vertex(0., 0., 400.), "b": vertex(600., 0., 400.)});
    for (id, p) in [
        ("sketch.create", json!({"plane": "XY", "name": "Base"})),
        ("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [600, 400]})),
        ("sketch.finish", json!({})),
        ("solid.extrude", json!({"distance": 400, "name": "Box", "body_name": "Box"})),
        ("frame.member", json!({"profile": tube(), "pairs": [rail, top]})),
    ] {
        h.app.run(id, p).unwrap();
    }
    let f = h.app.session.doc.features.last().unwrap().id;
    let stored = match &h.app.session.doc.feature(f).unwrap().kind {
        FeatureKind::FrameMember { pairs, .. } => serde_json::to_value(pairs).unwrap(),
        _ => panic!(),
    };
    h.app.dialog = Some(edit_dialog(&h.app, f, None).unwrap());
    h.frames(2);
    let (p0, p1, p2) = (corner(0., 0., 0.), corner(0., 0., 400.), corner(600., 0., 400.));
    assert_eq!(path(h.app.dialog.as_ref().unwrap()), [p0.clone(), p1.clone(), p1.clone(), p2]);
    let m = member(h.app.dialog.as_mut().unwrap());
    let (refs, ids) = (m.refs.clone(), m.pair_ids.clone());
    assert_eq!(ids[1].as_deref(), Some("pair1"));
    assert_eq!(path_rows(&mut h), 2, "a row for each pair");
    h.step(&json!({"click": {"handle": "dialog:list:Path:1:remove"}})).unwrap();
    h.frames(2);
    let d = h.app.dialog.as_mut().unwrap();
    assert_eq!(path(d), [p0, p1], "both of the second pair's points go");
    assert_eq!(d.error, None, "nothing waits");
    let m = member(d);
    assert_eq!(m.refs, refs[..2]);
    assert_eq!(m.pair_ids, ids[..2]);
    let c = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    assert_eq!(c[0].1["params"]["pairs"], json!([stored[0]]), "the first pair is intact, id and references");
}

/// The Frame sketch's point at (x, y) (not the sketch origin).
fn point_at(app: &SolveApp, x: f64, y: f64) -> String {
    let sk = app.session.doc.sketch(sketch_id(app, "Frame")).unwrap();
    sk.points.iter().find(|p| p.id != "origin" && (p.pos.x - x).abs() < 1e-6 && (p.pos.y - y).abs() < 1e-6).unwrap().id.clone()
}

/// The Frame sketch's lines with an end at (x, y), in the order they were drawn.
fn lines_at(app: &SolveApp, x: f64, y: f64) -> Vec<String> {
    let sk = app.session.doc.sketch(sketch_id(app, "Frame")).unwrap();
    let at = |i: usize| sk.points.get(i).is_some_and(|p| (p.pos.x - x).abs() < 1e-6 && (p.pos.y - y).abs() < 1e-6);
    sk.curves.iter().filter(|c| c.kind.ends().is_some_and(|(a, b)| at(a) || at(b))).map(|c| c.id.clone()).collect()
}

/// Corner Type and Flip reach the command; None writes neither, and a Mitre, which has no Flip,
/// no flip.
#[test]
fn corner_type_and_flip_go_into_the_command() {
    let mut app = frame_app();
    let lines = frame_lines(&app);
    let mut d = dialog_on(&mut app, &lines);
    member(&mut d).corner = corner_type("none");
    let p = &apply_commands(&app, &d).unwrap()[0].1;
    assert!(p.get("corner").is_none() && p.get("flip").is_none() && p.get("corners").is_none(), "{p}");
    let m = member(&mut d);
    m.corner = corner_type("butt");
    m.flip = true;
    let c = apply_commands(&app, &d).unwrap();
    assert_eq!((&c[0].1["corner"], &c[0].1["flip"]), (&json!("butt"), &json!(true)), "{}", c[0].1);
    member(&mut d).corner = corner_type("mitre");
    let p = &apply_commands(&app, &d).unwrap()[0].1;
    assert!(p["corner"] == "mitre" && p.get("flip").is_none(), "{p}");
    run_all(&mut app, c);
    assert_eq!(body_names(&app).len(), 4);
}

/// A corner point picked into Corner Overrides is saved by the two lines that end there, with its
/// own type: a new pick takes the feature's type, unflipped.
#[test]
fn a_picked_corner_point_becomes_an_override() {
    let mut app = frame_app();
    let lines = frame_lines(&app);
    let frame = sketch_id(&app, "Frame");
    let mut d = dialog_on(&mut app, &lines);
    member(&mut d).corner = corner_type("butt");
    let corner = Sel::ExternalSketchPoint { sketch: frame, id: point_at(&app, 0.0, 0.0) };
    d.active = 2;
    d.pick(&app.session, corner.clone());
    assert_eq!(d.inputs[2].items, std::slice::from_ref(&corner), "{:?}", d.error);
    let members = lines_at(&app, 0.0, 0.0);
    assert_eq!(members.len(), 2);
    let c = apply_commands(&app, &d).unwrap();
    assert_eq!(c[0].1["corners"], json!([{"members": members, "kind": "butt", "flip": false}]));
    member(&mut d).overrides = vec![Override::new(corner, corner_type("mitre"))];
    let c = apply_commands(&app, &d).unwrap();
    assert_eq!(c[0].1["corners"], json!([{"members": members, "kind": "mitre", "flip": false}]), "a Mitre pair: today's order");
    run_all(&mut app, c);
    assert_eq!(body_names(&app).len(), 4);
}

/// A point where no two members end alone (the foot of a T, a point on a line's middle) is not
/// taken, and the dialog says why.
#[test]
fn a_point_that_is_no_corner_is_refused_at_the_pick() {
    let mut app = app_with(&[
        ("sketch.create", json!({"plane": "XY", "name": "Frame"})),
        ("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [600, 400]})),
        ("sketch.line", json!({"points": [[300, 0], [300, 400]], "ids": ["post"]})),
        ("sketch.point", json!({"point": [0, 200], "id": "middle"})),
        ("sketch.finish", json!({})),
    ]);
    let frame = sketch_id(&app, "Frame");
    let lines = frame_lines(&app);
    let mut d = dialog_on(&mut app, &lines);
    d.active = 2;
    for id in [point_at(&app, 300.0, 0.0), "middle".to_string()] {
        d.pick(&app.session, Sel::ExternalSketchPoint { sketch: frame, id: id.clone() });
        assert!(d.inputs[2].items.is_empty(), "{id}");
        let e = d.error.clone().unwrap_or_default();
        assert!(e.contains("members end"), "{id}: {e}");
    }
    let corner = Sel::ExternalSketchPoint { sketch: frame, id: point_at(&app, 600.0, 400.0) };
    d.pick(&app.session, corner.clone());
    assert_eq!((d.inputs[2].items.as_slice(), d.error.as_deref()), ([corner].as_slice(), None));
}

/// Edit Feature brings back Corner Type, Flip and each override, the override as the point of its
/// corner.
#[test]
fn an_edit_shows_the_corner_settings_and_overrides() {
    let mut app = frame_app();
    let members = lines_at(&app, 0.0, 0.0);
    let corners = json!([{"members": members, "corner": "mitre", "flip": false}]);
    app.run("frame.member", json!({"sketch": "Frame", "profile": tube(), "corner": "butt", "flip": true, "corners": corners})).unwrap();
    let f = app.session.doc.features.last().unwrap().id;
    let mut e = edit_dialog(&app, f, None).unwrap();
    let frame = sketch_id(&app, "Frame");
    let [Sel::ExternalSketchPoint { sketch, id }] = e.inputs[2].items.as_slice() else { panic!("{:?}", e.inputs[2].items) };
    let sk = app.session.doc.sketch(frame).unwrap();
    assert_eq!((*sketch, sk.points[sk.point_index(id).unwrap()].pos), (frame, solidcore_record::Vec2::new(0.0, 0.0)));
    assert!(e.extra.get("corners").is_none());
    let m = member(&mut e);
    assert_eq!((CORNER_TYPES[m.corner].0, m.flip), ("butt", true));
    assert_eq!(m.overrides.iter().map(|o| (CORNER_TYPES[o.corner].0, o.flip)).collect::<Vec<_>>(), [("mitre", false)]);
    let c = apply_commands(&app, &e).unwrap();
    assert_eq!(c[0].1["params"]["corners"], json!([{"members": saved_orders(&app, f)[0], "kind": "mitre"}]), "as saved: {}", c[0].1);
}

/// A harness window tall enough for the whole Frame Member card with its corner rows: in the
/// default one the card scrolls, and its OK and Cancel lie over the last rows.
fn tall_harness() -> crate::scenario::Harness {
    let mut h = crate::scenario::Harness::new();
    h.call("ui.resize", json!({"width": 1600, "height": 1400}));
    h.app.home.open = false;
    h
}

/// An override whose two lines no longer meet still opens for editing: it is kept, listed as a
/// Missing corner, and its row's bin removes it.
#[test]
fn an_edit_opens_with_an_override_whose_corner_has_gone() {
    let mut h = tall_harness();
    for (id, p) in frame_commands() {
        h.app.run(id, p).unwrap();
    }
    h.app.run("frame.member", json!({"sketch": "Frame", "profile": tube()})).unwrap();
    let lines = frame_lines(&h.app);
    let f = h.app.session.doc.features.last().unwrap().id;
    // The opposite sides of the rectangle never meet, as two lines a dimension edit moved apart.
    let gone = json!([{"members": [lines[0], lines[2]], "corner": "butt", "flip": true}]);
    let doc = std::sync::Arc::make_mut(&mut h.app.session.doc);
    let FeatureKind::FrameMember { corners, .. } = &mut doc.feature_mut(f).unwrap().kind else { panic!() };
    *corners = serde_json::from_value(gone.clone()).unwrap();
    let e = edit_dialog(&h.app, f, None).unwrap();
    assert!(e.inputs[2].items.is_empty());
    let gone = saved(&gone);
    assert_eq!(e.extra.get("corners"), Some(&gone));
    // OK keeps it (the feature then says the override is not at a corner).
    assert_eq!(apply_commands(&h.app, &e).unwrap()[0].1["params"]["corners"], gone);
    h.app.dialog = Some(e);
    h.frames(2);
    let bin = "dialog:button:Corner 1:×";
    assert!(crate::scenario::rect_of(bin).is_some(), "the missing corner has a row with a bin");
    h.step(&json!({"click": {"handle": bin}})).unwrap();
    h.frames(1);
    let d = h.app.dialog.as_ref().unwrap();
    assert!(d.extra.get("corners").is_none());
    assert!(apply_commands(&h.app, d).unwrap()[0].1["params"].get("corners").is_none());
}

/// An X brace's diagonals cross where no sketch point is, so the edit can't show their override as
/// a picked point: it is kept, and its row reads Crossing (it still applies), not Missing corner.
#[test]
fn an_edit_shows_a_kept_crossing_override_as_a_crossing() {
    let mut h = tall_harness();
    for (id, p) in [
        ("sketch.create", json!({"plane": "XY", "name": "Frame"})),
        ("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [600, 400]})),
        ("sketch.line", json!({"points": [[0, 0], [600, 400]], "ids": ["d1"]})),
        ("sketch.line", json!({"points": [[0, 400], [600, 0]], "ids": ["d2"]})),
        ("sketch.finish", json!({})),
    ] {
        h.app.run(id, p).unwrap();
    }
    let over = json!([{"members": ["d1", "d2"], "kind": "auto", "flip": true}]);
    h.app.run("frame.member", json!({"sketch": "Frame", "profile": tube(), "corner": "auto", "corners": over})).unwrap();
    let f = h.app.session.doc.features.last().unwrap().id;
    let e = edit_dialog(&h.app, f, None).unwrap();
    assert!(e.inputs[2].items.is_empty());
    // Saved as the order Flip gave: d2 stays whole.
    assert_eq!(e.extra.get("corners").and_then(|c| c[0].get("members")), Some(&json!([{"line": "d2"}, {"line": "d1"}])));
    h.app.dialog = Some(e);
    h.frames(2);
    let texts = drawn_texts(&h);
    assert!(texts.iter().any(|t| t == "Crossing") && !texts.iter().any(|t| t == "Missing corner"), "{texts:?}");
}

/// An override of a corner where a point-to-point member ends (#15705) can't show as a picked
/// sketch point: an edit keeps it as it is, its row says its type, and OK sends it back.
#[test]
fn an_edit_keeps_an_override_of_a_pair_members_corner() {
    let mut app = frame_app();
    // The bottom line, and a tube up the right side between its two corner points: they meet at (600, 0).
    let bottom = lines_at(&app, 0.0, 0.0).into_iter().find(|l| lines_at(&app, 600.0, 0.0).contains(l)).unwrap();
    let (a, b) = (point_at(&app, 600.0, 0.0), point_at(&app, 600.0, 400.0));
    let over = json!([{"members": [bottom, "pair1"], "corner": "mitre", "flip": false}]);
    app.run(
        "frame.member",
        json!({"sketch": "Frame", "lines": [bottom], "profile": tube(), "corners": over,
               "pairs": [{"a": {"sketch": "Frame", "point": a}, "b": {"sketch": "Frame", "point": b}}]}),
    )
    .unwrap();
    let f = app.session.doc.features.last().unwrap().id;
    let e = edit_dialog(&app, f, None).unwrap();
    assert!(e.inputs[2].items.is_empty());
    let over = saved(&over);
    assert_eq!(e.extra.get("corners"), Some(&over));
    assert_eq!(kept_corner_text(&over[0], false), "Mitre");
    assert_eq!(kept_corner_text(&json!({"members": [bottom, "l9"], "corner": "mitre"}), false), "Missing corner");
    assert_eq!(apply_commands(&app, &e).unwrap()[0].1["params"]["corners"], over);
}

/// Remove Tube drops the corner overrides naming the tube taken out, as `frame.remove_member` does: one kept
/// from the edited feature (the bottom line and pair1), and a picked row (the brace's corner when the brace goes).
#[test]
fn remove_tube_drops_the_overrides_naming_it() {
    let mut app = frame_app();
    let bottom = lines_at(&app, 0.0, 0.0).into_iter().find(|l| lines_at(&app, 600.0, 0.0).contains(l)).unwrap();
    let (a, b) = (point_at(&app, 600.0, 0.0), point_at(&app, 600.0, 400.0));
    let over = json!([{"members": [bottom, "pair1"], "corner": "mitre", "flip": false}]);
    app.run(
        "frame.member",
        json!({"sketch": "Frame", "lines": [bottom], "profile": tube(), "corners": over,
               "pairs": [{"a": {"sketch": "Frame", "point": a}, "b": {"sketch": "Frame", "point": b}}]}),
    )
    .unwrap();
    let f = app.session.doc.features.last().unwrap().id;
    app.edit_feature(f);
    settle_preview(&mut app);
    super::remove_tubes(&mut app, &["pair1".to_string()]);
    let d = app.dialog.clone().unwrap();
    let c = apply_commands(&app, &d).unwrap();
    assert!(c[0].1["params"].get("corners").is_none(), "{}", c[0].1);
    crate::dialogs::run_dialog(&mut app, &d).unwrap();
    // A picked row: the three-tube corner at (0, 0) goes with the brace.
    let mut app = braced_app();
    let corners = json!([{"members": lines_at(&app, 0.0, 0.0), "corner": "auto", "flip": true}]);
    app.run("frame.member", json!({"sketch": "Frame", "profile": tube(), "corner": "auto", "corners": corners})).unwrap();
    let f = app.session.doc.features.last().unwrap().id;
    app.edit_feature(f);
    settle_preview(&mut app);
    assert_eq!(open(&mut app).inputs[2].items.len(), 1);
    super::remove_tubes(&mut app, &["brace".to_string()]);
    let d = app.dialog.clone().unwrap();
    assert!(d.inputs[2].items.is_empty(), "{:?}", d.inputs[2].items);
    let c = apply_commands(&app, &d).unwrap();
    assert!(c[0].1["params"].get("corners").is_none(), "{}", c[0].1);
    crate::dialogs::run_dialog(&mut app, &d).unwrap();
}

/// A new Frame Member starts on None (members left overlapping, the command's own default) with Fit
/// Ends to Earlier Frames ticked, unflipped, with no overrides, even after one made with Butt; its
/// command sends no corner, and under None no fit_ends.
#[test]
fn a_new_frame_member_starts_on_none_with_fit_ends() {
    let mut app = frame_app();
    let lines = frame_lines(&app);
    let mut d = dialog_on(&mut app, &lines);
    assert_eq!(d.inputs[2].label, "Corner Overrides");
    let m = member(&mut d);
    assert_eq!((CORNER_TYPES[m.corner].0, m.flip, m.fit_ends, m.overrides.len()), ("none", false, true, 0));
    let p = &apply_commands(&app, &d).unwrap()[0].1;
    assert!(p.get("corner").is_none() && p.get("fit_ends").is_none(), "{p}");
    assert!(p.get("flip").is_none() && p.get("corners").is_none(), "{p}");
    member(&mut d).corner = corner_type("butt");
    let c = apply_commands(&app, &d).unwrap();
    run_all(&mut app, c);
    app.dialog = None;
    let mut d = dialog_on(&mut app, &lines);
    assert_eq!(CORNER_TYPES[member(&mut d).corner].0, "none");
}

/// Fit Ends to Earlier Frames unticked writes nothing, and under None, ticked or not, neither.
#[test]
fn fit_ends_unticked_writes_nothing() {
    let mut app = frame_app();
    let lines = frame_lines(&app);
    let mut d = dialog_on(&mut app, &lines);
    member(&mut d).corner = corner_type("auto");
    member(&mut d).fit_ends = false;
    let p = &apply_commands(&app, &d).unwrap()[0].1;
    assert!(p["corner"] == "auto" && p.get("fit_ends").is_none(), "{p}");
    let m = member(&mut d);
    (m.fit_ends, m.corner) = (true, corner_type("none"));
    let p = &apply_commands(&app, &d).unwrap()[0].1;
    assert!(p.get("corner").is_none() && p.get("fit_ends").is_none(), "{p}");
    member(&mut d).corner = corner_type("mitre");
    assert_eq!(apply_commands(&app, &d).unwrap()[0].1["fit_ends"], json!(true));
}

/// Flip is hidden under Auto (it has no one corner to act on) and shows under Butt; Fit Ends to
/// Earlier Frames shows under every type but None.
#[test]
fn flip_is_hidden_under_auto() {
    let mut h = tall_harness();
    for (id, p) in frame_commands() {
        h.app.run(id, p).unwrap();
    }
    let lines = frame_lines(&h.app);
    h.app.session.selection = lines.iter().map(|id| Sel::SketchCurve { id: id.clone(), sketch: None }).collect();
    h.app.start("frame.member");
    h.frames(2);
    member(h.app.dialog.as_mut().unwrap()).corner = corner_type("auto");
    h.frames(2);
    assert!(h.widgets("Flip").is_empty(), "no Flip under Auto");
    assert!(h.widget("Fit Ends to Earlier Frames").is_some());
    member(h.app.dialog.as_mut().unwrap()).corner = corner_type("butt");
    h.frames(2);
    assert!(!h.widgets("Flip").is_empty(), "Flip under Butt");
    member(h.app.dialog.as_mut().unwrap()).corner = corner_type("none");
    h.frames(2);
    assert!(h.widget("Fit Ends to Earlier Frames").is_none(), "nothing to fit under None");
}

/// Under an Auto frame an override row's Type lists Auto (a new pick starts on it), and its Flip
/// is enabled and saved. On a frame of another type, Auto isn't offered, and an Auto override
/// reads and saves as the frame's type (it builds as that type, spec A6).
#[test]
fn override_rows_offer_auto_and_flip_it() {
    let names = |c: &[(&str, &str)]| c.iter().map(|c| c.0.to_string()).collect::<Vec<_>>();
    assert_eq!(names(override_choices(corner_type("auto"))), names(&CORNER_TYPES));
    assert_eq!(names(override_choices(corner_type("mitre"))), ["none", "mitre", "butt"]);
    let (mut h, _) = overrides_dialog("auto");
    assert!(h.widget_near("Auto", "Corner 1").is_some());
    let [_, y] = h.widget("Corner 1").unwrap();
    let tree = h.output.accesskit_update.as_ref().unwrap();
    let on_row = |n: &egui::accesskit::Node| n.bounds().is_some_and(|b| (f64::midpoint(b.y0, b.y1) - y).abs() < 2.0);
    let flip = tree.nodes.iter().find(|(_, n)| n.label() == Some("Flip") && on_row(n));
    assert!(flip.is_some_and(|(_, n)| !n.is_disabled()), "Corner 1's Flip is enabled under Auto");
    member(h.app.dialog.as_mut().unwrap()).overrides[0].flip = true;
    let c = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    assert_eq!((&c[0].1["corners"][0]["kind"], &c[0].1["corners"][0]["flip"]), (&json!("auto"), &json!(true)), "{}", c[0].1);
    member(h.app.dialog.as_mut().unwrap()).corner = corner_type("mitre");
    h.frames(2);
    assert!(h.widget_near("Mitre", "Corner 1").is_some());
    let c = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    assert_eq!((&c[0].1["corners"][0]["kind"], c[0].1["corners"][0].get("flip")), (&json!("mitre"), Some(&json!(false))), "{}", c[0].1);
    // Back on Auto, the row is Auto again.
    member(h.app.dialog.as_mut().unwrap()).corner = corner_type("auto");
    let c = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    assert_eq!(c[0].1["corners"][0]["kind"], json!("auto"));
}

/// The 600 × 400 rectangle with a brace `brace` from corner to corner: three lines end at (0, 0)
/// and at (600, 400).
fn braced_app() -> SolveApp {
    app_with(&[
        ("sketch.create", json!({"plane": "XY", "name": "Frame"})),
        ("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [600, 400]})),
        ("sketch.line", json!({"points": [[0, 0], [600, 400]], "ids": ["brace"]})),
        ("sketch.finish", json!({})),
    ])
}

/// Under Auto, a point where three lines end is a corner to override, saved by all three; under
/// Mitre the same pick is refused, as stage 1 refuses it.
#[test]
fn under_auto_a_three_tube_point_is_an_override() {
    let mut app = braced_app();
    let frame = sketch_id(&app, "Frame");
    let lines = frame_lines(&app);
    let corner = Sel::ExternalSketchPoint { sketch: frame, id: point_at(&app, 0.0, 0.0) };
    let members = lines_at(&app, 0.0, 0.0);
    assert_eq!(members.len(), 3, "{members:?}");
    let mut d = dialog_on(&mut app, &lines);
    member(&mut d).corner = corner_type("auto");
    d.active = 2;
    d.pick(&app.session, corner.clone());
    assert_eq!(d.inputs[2].items, std::slice::from_ref(&corner), "{:?}", d.error);
    let c = apply_commands(&app, &d).unwrap();
    assert_eq!(c[0].1["corners"], json!([{"members": members, "kind": "auto", "flip": false}]), "{}", c[0].1);
    let mut d = dialog_on(&mut app, &lines);
    member(&mut d).corner = corner_type("mitre");
    d.active = 2;
    d.pick(&app.session, corner);
    assert!(d.inputs[2].items.is_empty());
    let e = d.error.clone().unwrap_or_default();
    assert!(e.contains("exactly two members end"), "{e}");
}

/// Under Auto, a corner picked where a point-to-point member ends too is saved with that pair's id, as
/// the build counts every member ending there (ruling A16), and the feature builds. An edit that adds a
/// pair names it by the id the command gives it.
#[test]
fn an_auto_override_names_the_pairs_ending_at_its_corner() {
    let mut app = app_with(&[
        ("sketch.create", json!({"plane": "XY", "name": "Frame"})),
        ("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [600, 400]})),
        ("sketch.point", json!({"point": [300, 200], "id": "mid"})),
        ("sketch.finish", json!({})),
    ]);
    let frame = sketch_id(&app, "Frame");
    let lines = frame_lines(&app);
    let at = |app: &SolveApp, x: f64, y: f64| Sel::SketchPoint { id: format!("{frame}:{}", point_at(app, x, y)) };
    let mid = Sel::SketchPoint { id: format!("{frame}:mid") };
    let mut d = dialog_on(&mut app, &lines);
    member(&mut d).corner = corner_type("auto");
    d.active = 0;
    pick_all(&app, &mut d, &[at(&app, 0.0, 0.0), mid.clone()]);
    assert_eq!((path(&d).len(), d.error.as_deref()), (6, None));
    d.active = 2;
    d.pick(&app.session, Sel::ExternalSketchPoint { sketch: frame, id: point_at(&app, 0.0, 0.0) });
    let c = apply_commands(&app, &d).unwrap();
    let mut members = lines_at(&app, 0.0, 0.0);
    members.push("pair1".into());
    assert_eq!(c[0].1["corners"], json!([{"members": members, "kind": "auto", "flip": false}]), "{}", c[0].1);
    run_all(&mut app, c);
    let f = app.session.doc.features.last().unwrap().id;
    // The edit adds a second pair from (600, 400): its id is pair2.
    let mut e = edit_dialog(&app, f, None).unwrap();
    e.active = 0;
    pick_all(&app, &mut e, &[at(&app, 600.0, 400.0), mid]);
    e.active = 2;
    e.pick(&app.session, Sel::ExternalSketchPoint { sketch: frame, id: point_at(&app, 600.0, 400.0) });
    let c = apply_commands(&app, &e).unwrap();
    let mut far = lines_at(&app, 600.0, 400.0);
    far.push("pair2".into());
    let corners = c[0].1["params"]["corners"].as_array().cloned().unwrap_or_default();
    assert!(corners.iter().any(|o| o["members"] == json!(far)), "{}", c[0].1);
    run_all(&mut app, c);
    assert_eq!(body_names(&app).len(), 6);
}

/// Edit Feature brings back Auto, Fit Ends to Earlier Frames and a three-tube override as the point
/// of its corner, and OK sends them back as they were. An override that names only two of the
/// three is no longer that corner: it is kept as a Missing corner.
#[test]
fn an_edit_shows_a_three_tube_override() {
    let mut app = braced_app();
    let members = lines_at(&app, 0.0, 0.0);
    let corners = json!([{"members": members, "corner": "auto", "flip": true}]);
    app.run("frame.member", json!({"sketch": "Frame", "profile": tube(), "corner": "auto", "fit_ends": true, "corners": corners})).unwrap();
    let f = app.session.doc.features.last().unwrap().id;
    let mut e = edit_dialog(&app, f, None).unwrap();
    let frame = sketch_id(&app, "Frame");
    let [Sel::ExternalSketchPoint { sketch, id }] = e.inputs[2].items.as_slice() else { panic!("{:?} {:?}", e.inputs[2].items, e.extra) };
    let sk = app.session.doc.sketch(frame).unwrap();
    assert_eq!((*sketch, sk.points[sk.point_index(id).unwrap()].pos), (frame, solidcore_record::Vec2::new(0.0, 0.0)));
    assert!(e.extra.get("corners").is_none());
    let m = member(&mut e);
    assert_eq!((CORNER_TYPES[m.corner].0, m.fit_ends), ("auto", true));
    assert_eq!(m.overrides.iter().map(|o| (CORNER_TYPES[o.corner].0, o.flip)).collect::<Vec<_>>(), [("auto", true)]);
    let p = &apply_commands(&app, &e).unwrap()[0].1["params"];
    let kept = json!([{"members": saved_orders(&app, f)[0], "kind": "auto"}]);
    assert_eq!((&p["corner"], &p["fit_ends"], &p["corners"]), (&json!("auto"), &json!(true), &kept), "as saved: {p}");
    // Two of the three: the set of tubes there changed, so it no longer matches.
    let two = json!([{"members": members[..2], "corner": "auto", "flip": false}]);
    let doc = std::sync::Arc::make_mut(&mut app.session.doc);
    let FeatureKind::FrameMember { corners, .. } = &mut doc.feature_mut(f).unwrap().kind else { panic!() };
    *corners = serde_json::from_value(two.clone()).unwrap();
    let e = edit_dialog(&app, f, None).unwrap();
    assert!(e.inputs[2].items.is_empty(), "{:?}", e.inputs[2].items);
    assert_eq!(e.extra.get("corners"), Some(&saved(&two)));
}

/// Corner overrides as a design saves them (#17854: `{members: [{line}], kind}`, an old `flip` dropped).
fn saved(corners: &Value) -> Value {
    serde_json::to_value(serde_json::from_value::<Vec<solidcraft_engine::doc::frame::CornerOverride>>(corners.clone()).unwrap()).unwrap()
}

/// The Gap fields on the row under the one at height `y`: whether each is greyed.
fn gaps_under(h: &crate::scenario::Harness, y: f64) -> Vec<bool> {
    let tree = h.output.accesskit_update.as_ref().unwrap();
    let row = f64::from(solidcraft_theme::Tokens::get().control_height);
    tree.nodes
        .iter()
        .filter(|(_, n)| n.role() == egui::accesskit::Role::TextInput && n.label() == Some("Gap"))
        .filter(|(_, n)| n.bounds().is_some_and(|b| (2.0..1.5 * row).contains(&(f64::midpoint(b.y0, b.y1) - y))))
        .map(|(_, n)| n.is_disabled())
        .collect()
}

/// Gap (#17854) is a row of its own under Fit Ends and Coped: typed, it goes into the command as `gap`; 0 or empty
/// sends nothing; under None it is greyed and sends nothing. An edit brings it back.
#[test]
fn the_gap_field_is_sent() {
    let (mut h, _) = overrides_dialog("butt");
    let [_, cy] = h.widget("Coped").unwrap();
    assert_eq!(gaps_under(&h, cy), [false], "one enabled Gap under Coped");
    let sent = |h: &crate::scenario::Harness| apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap()[0].1.get("gap").cloned();
    assert_eq!(sent(&h), None, "Gap starts empty: nothing sent");
    for (typed, expected) in [("3 mm", Some(json!("3 mm"))), ("0", None), ("0 mm", None), ("", None)] {
        member(h.app.dialog.as_mut().unwrap()).gap = typed.into();
        assert_eq!(sent(&h), expected, "Gap {typed:?}");
    }
    member(h.app.dialog.as_mut().unwrap()).gap = "3 mm".into();
    member(h.app.dialog.as_mut().unwrap()).corner = corner_type("none");
    h.frames(2);
    let [_, cy] = h.widget("Coped").unwrap();
    assert_eq!(gaps_under(&h, cy), [true], "Gap is greyed under None");
    assert_eq!(sent(&h), None, "under None no gap is sent");
    member(h.app.dialog.as_mut().unwrap()).corner = corner_type("butt");
    let c = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    h.app.dialog = None;
    run_all(&mut h.app, c);
    let f = h.app.session.doc.features.last().unwrap().id;
    let mut e = edit_dialog(&h.app, f, None).unwrap();
    assert_eq!(member(&mut e).gap, "3 mm");
}

/// Each Corner Overrides row has a Gap row of its own under it (empty: the frame's): a row sends its `kind` and its
/// gap, a Mitre row (Flip greyed) `flip` false, a Butt row its own. A kept override of tubes that overlap has no Gap.
#[test]
fn an_override_row_sends_its_gap_and_kind() {
    let (mut h, _) = overrides_dialog("butt");
    for n in [1, 2] {
        let [_, y] = h.widget(&format!("Corner {n}")).unwrap();
        assert_eq!(gaps_under(&h, y), [false], "Corner {n} has a Gap");
    }
    let m = member(h.app.dialog.as_mut().unwrap());
    m.overrides[0].corner = corner_type("mitre");
    m.overrides[0].gap = "2 mm".into();
    let c = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    let members = lines_at(&h.app, 0.0, 0.0);
    assert_eq!(c[0].1["corners"][0], json!({"members": members, "kind": "mitre", "flip": false, "gap": "2 mm"}), "{}", c[0].1);
    assert_eq!(c[0].1["corners"][1], json!({"members": lines_at(&h.app, 600.0, 400.0), "kind": "butt", "flip": false}), "{}", c[0].1);
    h.app.dialog = None;
    run_all(&mut h.app, c);
    let f = h.app.session.doc.features.last().unwrap().id;
    let mut e = edit_dialog(&h.app, f, None).unwrap();
    assert_eq!(member(&mut e).overrides.iter().map(|o| o.gap.as_str()).collect::<Vec<_>>(), ["2 mm", ""]);

    // A post whose foot stops 15 mm short of the bottom rail: its tube overlaps the rail's.
    let mut h = tall_harness();
    for (id, p) in [
        ("sketch.create", json!({"plane": "XY", "name": "Frame"})),
        ("sketch.line", json!({"points": [[0, 0], [600, 0]], "ids": ["rail"]})),
        ("sketch.line", json!({"points": [[300, 15], [300, 300]], "ids": ["post"]})),
        ("sketch.finish", json!({})),
    ] {
        h.app.run(id, p).unwrap();
    }
    let over = json!([{"members": ["rail", "post"], "kind": "auto"}]);
    h.app.run("frame.member", json!({"sketch": "Frame", "profile": tube(), "corner": "auto", "corners": over})).unwrap();
    let f = h.app.session.doc.features.last().unwrap().id;
    let e = edit_dialog(&h.app, f, None).unwrap();
    assert!(e.inputs[2].items.is_empty() && e.extra.get("corners").is_some(), "kept: {:?}", e.extra);
    h.app.dialog = Some(e);
    h.frames(2);
    let [_, y] = h.widget("Corner 1").unwrap();
    assert!(gaps_under(&h, y).is_empty(), "the overlap row has no Gap");
}

/// The Butt frame on the rectangle with an override at (0, 0) saved in today's order and one at (600, 400) saved
/// the other way round (`flip` makes those orders), and the lines of each.
fn flipped_frame(app: &mut SolveApp) -> [Vec<String>; 2] {
    let (near, far) = (lines_at(app, 0.0, 0.0), lines_at(app, 600.0, 400.0));
    let corners = json!([{"members": near, "kind": "butt", "flip": false}, {"members": far, "kind": "butt", "flip": true}]);
    app.run("frame.member", json!({"sketch": "Frame", "profile": tube(), "corner": "butt", "corners": corners})).unwrap();
    [near, far]
}

/// The saved order of each override of Frame Member `f`.
fn saved_orders(app: &SolveApp, f: u64) -> Vec<Vec<String>> {
    let FeatureKind::FrameMember { corners, .. } = &app.session.doc.feature(f).unwrap().kind else { panic!() };
    corners.iter().map(|c| c.lines().map(String::from).collect()).collect()
}

/// Reopened, a row shows Flip ticked when its saved order is not the one today's rules give (`default_order`), and
/// unticked when it is.
#[test]
fn reopening_shows_flip_from_the_saved_order() {
    let mut app = frame_app();
    let [near, far] = flipped_frame(&mut app);
    let f = app.session.doc.features.last().unwrap().clone();
    let st = app.session.model.state_before(f.id);
    let default = |lines: &[String]| frame::default_order(&app.session.doc, &st, &f, lines).unwrap();
    let (near, mut far) = (default(&near), default(&far));
    far.swap(0, 1);
    assert_eq!(saved_orders(&app, f.id), [near, far], "one in today's order, one the other way round");
    let mut e = edit_dialog(&app, f.id, None).unwrap();
    assert_eq!(member(&mut e).overrides.iter().map(|o| o.flip).collect::<Vec<_>>(), [false, true]);
}

/// Two Frame Members alike in every setting: editing the second reads its Flips against the model before it, not
/// the first one's (which, moved before its sketch, has no lines to order).
#[test]
fn an_edit_reads_flip_for_its_own_frame_of_two_alike() {
    let mut app = frame_app();
    flipped_frame(&mut app);
    flipped_frame(&mut app);
    let doc = std::sync::Arc::make_mut(&mut app.session.doc);
    let n = doc.features.len();
    // Alike down to the names their tubes keep.
    doc.features[n - 1].kind = doc.features[n - 2].kind.clone();
    let (first, second) = (doc.features[n - 2].clone(), doc.features[n - 1].clone());
    // The first Frame Member before every sketch.
    doc.features.remove(n - 2);
    doc.features.insert(0, first);
    app.session.refresh();
    let mut e = edit_dialog(&app, second.id, None).unwrap();
    assert_eq!(member(&mut e).overrides.iter().map(|o| o.flip).collect::<Vec<_>>(), [false, true]);
}

/// OK on an edit leaves an override whose Flip was not touched exactly as it was saved: a three-tube corner whose
/// first two are not the pair today's rules give keeps its order (a Flip would only swap today's first two).
/// Unticking Flip sends `flip` against today's order.
#[test]
fn an_untouched_override_keeps_its_saved_order() {
    let mut app = braced_app();
    let members = lines_at(&app, 0.0, 0.0);
    app.run("frame.member", json!({"sketch": "Frame", "profile": tube(), "corner": "auto"})).unwrap();
    let f = app.session.doc.features.last().unwrap().clone();
    let st = app.session.model.state_before(f.id);
    let today = frame::default_order(&app.session.doc, &st, &f, &members).unwrap();
    let order = vec![today[0].clone(), today[2].clone(), today[1].clone()];
    let corners = json!([{"members": order, "kind": "auto"}]);
    let params = json!({"sketch": "Frame", "profile": tube(), "corner": "auto", "corners": corners});
    app.run("timeline.redefine", json!({"feature": f.id, "command": "frame.member", "params": params})).unwrap();
    assert_eq!(saved_orders(&app, f.id), std::slice::from_ref(&order));
    let mut e = edit_dialog(&app, f.id, None).unwrap();
    assert_eq!(member(&mut e).overrides.iter().map(|o| o.flip).collect::<Vec<_>>(), [true], "not today's first two");
    let c = apply_commands(&app, &e).unwrap();
    assert_eq!(c[0].1["params"]["corners"], json!([{"members": order, "kind": "auto"}]), "{}", c[0].1);
    run_all(&mut app, c);
    assert_eq!(saved_orders(&app, f.id), std::slice::from_ref(&order), "OK changed nothing");
    // Unticked: today's order.
    let mut e = edit_dialog(&app, f.id, None).unwrap();
    member(&mut e).overrides[0].flip = false;
    let c = apply_commands(&app, &e).unwrap();
    assert_eq!(c[0].1["params"]["corners"][0]["flip"], json!(false), "{}", c[0].1);
    run_all(&mut app, c);
    assert_eq!(saved_orders(&app, f.id), [today]);
}

/// A new Mitre row (Flip greyed) is sent with `flip` false, so it is saved in today's order, not as the corner lists
/// its lines: reopened, its Flip is unticked, and a later switch to Butt runs today's member through.
#[test]
fn a_new_mitre_row_is_saved_in_todays_order() {
    let (mut h, _) = overrides_dialog("butt");
    for o in &mut member(h.app.dialog.as_mut().unwrap()).overrides {
        o.corner = corner_type("mitre");
    }
    let c = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    assert!(c[0].1["corners"].as_array().unwrap().iter().all(|e| e["flip"] == json!(false)), "{}", c[0].1);
    h.app.dialog = None;
    run_all(&mut h.app, c);
    let f = h.app.session.doc.features.last().unwrap().clone();
    let st = h.app.session.model.state_before(f.id);
    let today: Vec<Vec<String>> = saved_orders(&h.app, f.id).iter().map(|o| frame::default_order(&h.app.session.doc, &st, &f, o).unwrap()).collect();
    assert_eq!(saved_orders(&h.app, f.id), today);
    let mut e = edit_dialog(&h.app, f.id, None).unwrap();
    assert_eq!(member(&mut e).overrides.iter().map(|o| o.flip).collect::<Vec<_>>(), [false, false]);
}

/// With Corner Overrides active, a corner of the frame sketch (not being edited) is picked in the
/// view as that sketch's point.
#[test]
fn a_corner_is_picked_in_the_view() {
    let mut h = crate::scenario::Harness::new();
    h.app.home.open = false;
    for (id, p) in frame_commands() {
        h.app.run(id, p).unwrap();
    }
    h.app.set_origin_shown(false);
    h.call("ui.view", json!({"view": "top"}));
    h.call("ui.view", json!({"view": "fit"}));
    h.app.start("frame.member");
    h.app.dialog.as_mut().unwrap().active = 2;
    h.frames(2);
    let [x, y] = h.at(&json!({"world": [600, 400, 0]})).unwrap();
    let proj = crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap());
    let hits = crate::viewport::pick(&h.app, &proj, egui::pos2(x as f32, y as f32));
    let d = h.app.dialog.as_ref().unwrap();
    let picked: Vec<Sel> = hits.iter().filter_map(|hit| d.candidate(&h.app.session, hit)).collect();
    let frame = sketch_id(&h.app, "Frame");
    assert!(picked.contains(&Sel::ExternalSketchPoint { sketch: frame, id: point_at(&h.app, 600.0, 400.0) }), "{picked:?} from {hits:?}");
}

/// A marquee over the frame sketch picks its straight lines into Lines (not a construction line or
/// an arc drawn inside it), and its corners into Corner Overrides.
#[test]
fn a_box_over_the_frame_sketch_picks_its_lines() {
    let mut h = crate::scenario::Harness::new();
    h.app.home.open = false;
    for (id, p) in frame_commands() {
        h.app.run(id, p).unwrap();
    }
    let mut want = frame_lines(&h.app);
    want.sort();
    for (id, p) in [
        ("sketch.edit", json!({"sketch": "Frame"})),
        ("sketch.line", json!({"points": [[100, 100], [500, 100]], "construction": true})),
        ("sketch.arc.three_point", json!({"start": [100, 300], "through": [300, 350], "end": [500, 300]})),
        ("sketch.finish", json!({})),
    ] {
        h.app.run(id, p).unwrap();
    }
    h.app.set_origin_shown(false);
    h.app.ui.show_browser = false;
    h.app.ui.show_timeline = false;
    h.call("ui.view", json!({"view": "top"}));
    h.call("ui.camera", json!({"target": [300, 200, 0], "distance": 1600}));
    h.app.start("frame.member");
    h.frames(2);
    let frame = sketch_id(&h.app, "Frame");
    let drag = json!({"drag": [{"world": [-50, -50, 0]}, {"world": [650, 450, 0]}]});
    h.step(&drag).unwrap();
    let mut lines: Vec<String> = h.app.dialog.as_ref().unwrap().inputs[0]
        .items
        .iter()
        .filter_map(|s| match s {
            Sel::ExternalSketchCurve { sketch, id } if *sketch == frame => Some(id.clone()),
            _ => None,
        })
        .collect();
    lines.sort();
    assert_eq!(lines, want, "{:?}", h.app.dialog.as_ref().unwrap().inputs[0].items);
    // With Corner Overrides active the same marquee takes the 4 corners.
    h.app.dialog.as_mut().unwrap().active = 2;
    h.frames(2);
    h.step(&drag).unwrap();
    let corners = &h.app.dialog.as_ref().unwrap().inputs[2].items;
    assert_eq!(corners.len(), 4, "{corners:?}");
    assert!(corners.contains(&Sel::ExternalSketchPoint { sketch: frame, id: point_at(&h.app, 600.0, 400.0) }), "{corners:?}");
}

/// A marquee over a 3D sketch picks its straight lines into Lines, as a click on each does.
#[test]
fn a_box_over_a_3d_sketch_picks_its_lines() {
    let mut h = crate::scenario::Harness::new();
    h.app.home.open = false;
    for (id, p) in [
        ("sketch.create", json!({"plane": "XY", "name": "Space"})),
        ("sketch.options", json!({"sketch_3d": true})),
        ("sketch.line3d", json!({"points": [[0, 0, 0], [600, 0, 0]]})),
        ("sketch.line3d", json!({"points": [[600, 0, 0], [600, 400, 0]]})),
        ("sketch.finish", json!({})),
    ] {
        h.app.run(id, p).unwrap();
    }
    let space = sketch_id(&h.app, "Space");
    let mut want: Vec<String> = h.app.session.doc.sketch(space).unwrap().wires.iter().map(|w| w.id.clone()).collect();
    want.sort();
    assert_eq!(want.len(), 2, "{want:?}");
    h.app.set_origin_shown(false);
    h.app.ui.show_browser = false;
    h.app.ui.show_timeline = false;
    h.call("ui.view", json!({"view": "top"}));
    h.call("ui.camera", json!({"target": [300, 200, 0], "distance": 1600}));
    h.app.start("frame.member");
    h.frames(2);
    h.step(&json!({"drag": [{"world": [-50, -50, 0]}, {"world": [650, 450, 0]}]})).unwrap();
    let mut lines: Vec<String> = h.app.dialog.as_ref().unwrap().inputs[0]
        .items
        .iter()
        .filter_map(|s| match s {
            Sel::ExternalSketchCurve { sketch, id } if *sketch == space => Some(id.clone()),
            _ => None,
        })
        .collect();
    lines.sort();
    assert_eq!(lines, want, "{:?}", h.app.dialog.as_ref().unwrap().inputs[0].items);
}

/// The frame dialog in a harness, the frame's lines picked, the feature's type `corner`, and the
/// rectangle's corners at (0, 0) and (600, 400) picked into Corner Overrides.
fn overrides_dialog(corner: &str) -> (crate::scenario::Harness, [Sel; 2]) {
    let mut h = tall_harness();
    for (id, p) in frame_commands() {
        h.app.run(id, p).unwrap();
    }
    let lines = frame_lines(&h.app);
    h.app.session.selection = lines.iter().map(|id| Sel::SketchCurve { id: id.clone(), sketch: None }).collect();
    h.app.start("frame.member");
    let frame = sketch_id(&h.app, "Frame");
    let picks = [(0.0, 0.0), (600.0, 400.0)].map(|(x, y)| Sel::ExternalSketchPoint { sketch: frame, id: point_at(&h.app, x, y) });
    let d = h.app.dialog.as_mut().unwrap();
    member(d).corner = corner_type(corner);
    d.active = 2;
    for p in &picks {
        h.app.dialog.as_mut().unwrap().pick(&h.app.session, p.clone());
    }
    h.frames(2);
    (h, picks)
}

/// A corner taken out of Corner Overrides forgets its own type and flip: picked again, it starts
/// as the feature's type, unflipped.
#[test]
fn a_corner_picked_again_starts_as_the_features_type() {
    let (mut h, [corner, _]) = overrides_dialog("mitre");
    let d = h.app.dialog.as_mut().unwrap();
    let o = member(d).overrides.iter_mut().find(|o| o.pick == corner).unwrap();
    (o.corner, o.flip) = (corner_type("butt"), true);
    d.inputs[2].items.retain(|x| *x != corner);
    h.frames(1);
    h.app.dialog.as_mut().unwrap().pick(&h.app.session, corner.clone());
    h.frames(1);
    let m = member(h.app.dialog.as_mut().unwrap());
    let o = m.overrides.iter().find(|o| o.pick == corner).unwrap();
    assert_eq!((CORNER_TYPES[o.corner].0, o.flip), ("mitre", false), "{:?}", m.overrides);
}

/// Each override is one row: "Corner n" (in pick order), its type, its Flip (greyed for None and
/// Mitre so the rows line up) and its bin, which removes that corner alone.
#[test]
fn each_corner_override_is_one_row() {
    let (mut h, picks) = overrides_dialog("butt");
    let d = h.app.dialog.as_mut().unwrap();
    member(d).overrides[0].corner = corner_type("mitre");
    h.frames(2);
    let flips = h.widgets("Flip");
    for n in [1, 2] {
        let bin = crate::scenario::rect_of(&format!("dialog:button:Corner {n}:×")).unwrap_or_else(|| panic!("Corner {n} has a bin"));
        let [_, y] = h.widget(&format!("Corner {n}")).unwrap_or_else(|| panic!("a row labelled Corner {n}"));
        assert!((y - f64::from(bin.center().y)).abs() < 2.0, "Corner {n}: label at {y}, bin at {bin:?}");
        assert!(flips.iter().any(|f| (f[1] - y).abs() < 2.0), "Corner {n}: a Flip on its row: {flips:?} vs {y}");
    }
    assert!(h.widgets("Type").is_empty(), "no separate Type row");
    // Corner 2's type reads as its own: the feature's Butt.
    assert!(h.widget_near("Butt", "Corner 2").is_some());
    let bin = "dialog:button:Corner 2:×";
    h.step(&json!({"click": {"handle": bin}})).unwrap();
    h.frames(1);
    assert_eq!(h.app.dialog.as_ref().unwrap().inputs[2].items, std::slice::from_ref(&picks[0]));
}

/// Flip shows in full on every override row (never "Fl…"), and the bin sits at the row's right edge.
#[test]
fn a_corner_override_row_shows_flip_in_full_with_the_bin_at_the_right() {
    let (mut h, _) = overrides_dialog("butt");
    h.frames(2);
    let drawn: Vec<String> =
        h.shapes.iter().filter_map(|s| if let egui::Shape::Text(t) = &s.shape { Some(t.galley.text().to_owned()) } else { None }).collect();
    assert!(!drawn.iter().any(|t| t.starts_with("Fl") && t != "Flip"), "an elided Flip: {drawn:?}");
    assert_eq!(drawn.iter().filter(|t| *t == "Flip").count(), 3, "{drawn:?}");
    let t = solidcraft_theme::Tokens::get();
    let need = t.space_14
        + t.space_6
        + h.ctx.fonts_mut(|f| f.layout_no_wrap("Flip".to_owned(), egui::FontId::proportional(t.font_label), egui::Color32::WHITE).size().x);
    let tree = h.output.accesskit_update.as_ref().unwrap();
    let flips: Vec<_> = tree.nodes.iter().filter(|(_, n)| n.label() == Some("Flip")).filter_map(|(_, n)| n.bounds()).collect();
    for n in [1, 2] {
        let [_, y] = h.widget(&format!("Corner {n}")).unwrap();
        let f = flips.iter().find(|b| (f64::midpoint(b.y0, b.y1) - y).abs() < 2.0).unwrap_or_else(|| panic!("Corner {n}: a Flip on its row"));
        assert!(f.x1 - f.x0 + 0.5 >= f64::from(need), "Corner {n}: Flip is {} wide, its text needs {need}", f.x1 - f.x0);
    }
    for n in [1, 2] {
        let row = crate::scenario::rect_of(&format!("dialog:row:Corner {n}")).unwrap();
        let bin = crate::scenario::rect_of(&format!("dialog:button:Corner {n}:×")).unwrap();
        assert!((0.0..2.0).contains(&(row.right() - bin.right())), "Corner {n}: bin {bin:?} at the right of {row:?}");
    }
}

/// Corner Type offers Auto, None, Mitre and Butt, and an override row None, Mitre and Butt: Coped (#17854) is a tick
/// box of its own on the Fit Ends to Earlier Frames row, to the right of that box, unticked at the start and greyed
/// under None.
#[test]
fn coped_is_a_tick_box_beside_fit_ends() {
    assert_eq!(CORNER_TYPES.map(|c| c.0)[..], ["auto", "none", "mitre", "butt"]);
    let (mut h, _) = overrides_dialog("butt");
    assert!(!member(h.app.dialog.as_mut().unwrap()).coped);
    h.frames(2);
    // The Coped box: its centre, and whether it is greyed.
    let coped = |h: &crate::scenario::Harness| {
        let tree = h.output.accesskit_update.as_ref().unwrap();
        let boxes: Vec<_> =
            tree.nodes.iter().filter(|(_, n)| n.label() == Some("Coped")).filter_map(|(_, n)| Some((n.bounds()?, n.is_disabled()))).collect();
        assert_eq!(boxes.len(), 1, "one Coped: {boxes:?}");
        let (b, disabled) = boxes[0];
        ([f64::midpoint(b.x0, b.x1), f64::midpoint(b.y0, b.y1)], disabled)
    };
    let fit = h.widgets("Fit Ends to Earlier Frames");
    let ([x, y], disabled) = coped(&h);
    assert!(!disabled, "Coped is enabled under Butt");
    assert!(fit.len() >= 2 && fit.iter().all(|f| (f[1] - y).abs() < 2.0 && f[0] < x), "Coped at {x}, {y}: Fit Ends at {fit:?}");
    // No override row offers Coped.
    assert!(h.widget_near("Coped", "Corner 1").is_none_or(|[cx, cy]| (cx, cy) == (x, y)));
    member(h.app.dialog.as_mut().unwrap()).corner = corner_type("none");
    h.frames(2);
    assert!(coped(&h).1, "Coped is greyed under None");
    assert!(h.widget("Fit Ends to Earlier Frames").is_none());
}

/// Coped ticked puts `"coped": true` in the command under every type but None (unticked or under None, nothing), and
/// Edit Feature brings it back ticked.
#[test]
fn coped_goes_into_the_command_and_an_edit_brings_it_back() {
    let mut app = frame_app();
    let lines = frame_lines(&app);
    let mut d = dialog_on(&mut app, &lines);
    member(&mut d).corner = corner_type("butt");
    assert!(apply_commands(&app, &d).unwrap()[0].1.get("coped").is_none());
    member(&mut d).coped = true;
    let c = apply_commands(&app, &d).unwrap();
    assert_eq!((&c[0].1["corner"], &c[0].1["coped"]), (&json!("butt"), &json!(true)), "{}", c[0].1);
    member(&mut d).corner = corner_type("none");
    let p = &apply_commands(&app, &d).unwrap()[0].1;
    assert!(p.get("corner").is_none() && p.get("coped").is_none(), "{p}");
    run_all(&mut app, c);
    let f = app.session.doc.features.last().unwrap().id;
    let mut e = edit_dialog(&app, f, None).unwrap();
    let m = member(&mut e);
    assert_eq!((CORNER_TYPES[m.corner].0, m.coped), ("butt", true));
    let p = &apply_commands(&app, &e).unwrap()[0].1["params"];
    assert_eq!((&p["corner"], &p["coped"]), (&json!("butt"), &json!(true)), "{p}");
}

/// The Corner Overrides line's arrow folds the corner rows away and back; a new pick opens them.
#[test]
fn the_corner_override_rows_fold() {
    let (mut h, _) = overrides_dialog("butt");
    let rows = || [1, 2].map(|n| crate::scenario::rect_of(&format!("dialog:button:Corner {n}:×")).is_some());
    assert_eq!(rows(), [true, true], "open at the start");
    let toggle = "dialog:list_toggle:Corner Overrides";
    h.step(&json!({"click": {"handle": toggle}})).unwrap();
    h.frames(1);
    assert_eq!(rows(), [false, false], "folded");
    assert!(crate::scenario::handle_at(toggle).is_some(), "the arrow stays to open them again");
    h.step(&json!({"click": {"handle": toggle}})).unwrap();
    h.frames(1);
    assert_eq!(rows(), [true, true], "open again");
    h.step(&json!({"click": {"handle": toggle}})).unwrap();
    h.frames(1);
    let corner = Sel::ExternalSketchPoint { sketch: sketch_id(&h.app, "Frame"), id: point_at(&h.app, 600.0, 0.0) };
    h.app.dialog.as_mut().unwrap().pick(&h.app.session, corner);
    h.frames(2);
    assert!(crate::scenario::rect_of("dialog:button:Corner 3:×").is_some(), "a new pick opens the rows");
}

/// The pointer over an overridden corner in the view lights that corner's row (the Corner
/// Overrides line while the rows are folded); over a corner with no override, nothing.
#[test]
fn hovering_an_overridden_corner_lights_its_row() {
    let (mut h, _) = overrides_dialog("butt");
    h.app.set_origin_shown(false);
    h.call("ui.view", json!({"view": "top"}));
    h.call("ui.view", json!({"view": "fit"}));
    h.frames(2);
    let lit = || crate::scenario::rects("dialog:lit:").into_iter().map(|r| r.0).collect::<Vec<_>>();
    h.step(&json!({"move": {"world": [600, 400, 0]}})).unwrap();
    h.frames(2);
    assert_eq!(lit(), ["dialog:lit:Corner 2"]);
    h.step(&json!({"move": {"world": [600, 0, 0]}})).unwrap();
    h.frames(2);
    assert!(lit().is_empty(), "{:?}", lit());
    h.step(&json!({"click": {"handle": "dialog:list_toggle:Corner Overrides"}})).unwrap();
    h.step(&json!({"move": {"world": [600, 400, 0]}})).unwrap();
    h.frames(2);
    assert_eq!(lit(), ["dialog:lit:Corner Overrides"]);
}

/// Under Auto a sketch point where two of the frame's lines cross (none ending there) is an
/// override, saved by the lines crossing there. Its row reads Crossing, with Flip and no Type
/// list, and an edit brings it back. Under None, which cuts no crossing, the same pick is refused.
#[test]
fn a_crossing_point_override_is_a_crossing_row_with_flip() {
    let mut h = tall_harness();
    for (id, p) in [
        ("sketch.create", json!({"plane": "XY", "name": "Frame"})),
        ("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [600, 400]})),
        ("sketch.line", json!({"points": [[0, 0], [600, 400]], "ids": ["d1"]})),
        ("sketch.line", json!({"points": [[0, 400], [600, 0]], "ids": ["d2"]})),
        ("sketch.point", json!({"point": [300, 200], "id": "x"})),
        ("sketch.finish", json!({})),
    ] {
        h.app.run(id, p).unwrap();
    }
    let frame = sketch_id(&h.app, "Frame");
    let lines = frame_lines(&h.app);
    let x = Sel::ExternalSketchPoint { sketch: frame, id: "x".into() };
    let mut d = dialog_on(&mut h.app, &lines);
    member(&mut d).corner = corner_type("auto");
    d.active = 2;
    d.pick(&h.app.session, x.clone());
    assert_eq!(d.inputs[2].items, std::slice::from_ref(&x), "{:?}", d.error);
    let c = apply_commands(&h.app, &d).unwrap();
    assert_eq!(c[0].1["corners"], json!([{"members": ["d1", "d2"], "kind": "auto", "flip": false}]), "{}", c[0].1);
    h.app.dialog = Some(d);
    h.frames(2);
    assert!(h.widget_near("Crossing", "Corner 1").is_some(), "the row reads Crossing");
    let [_, y] = h.widget("Corner 1").unwrap();
    let tree = h.output.accesskit_update.as_ref().unwrap();
    let on_row: Vec<_> = tree
        .nodes
        .iter()
        .filter(|(_, n)| n.bounds().is_some_and(|b| b.x1 > b.x0 && (f64::midpoint(b.y0, b.y1) - y).abs() < 2.0))
        .map(|(_, n)| n.clone())
        .collect();
    assert!(on_row.iter().any(|n| n.label() == Some("Flip") && !n.is_disabled()), "an enabled Flip on the row");
    assert!(!on_row.iter().any(|n| n.label() == Some("Auto") || n.value() == Some("Auto")), "no Type list on the row");
    member(h.app.dialog.as_mut().unwrap()).overrides[0].flip = true;
    let c = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    assert_eq!(c[0].1["corners"], json!([{"members": ["d1", "d2"], "kind": "auto", "flip": true}]), "{}", c[0].1);
    run_all(&mut h.app, c);
    let f = h.app.session.doc.features.last().unwrap().id;
    let mut e = edit_dialog(&h.app, f, None).unwrap();
    assert_eq!(e.inputs[2].items, std::slice::from_ref(&x), "{:?}", e.extra);
    assert!(e.extra.get("corners").is_none());
    assert_eq!(member(&mut e).overrides.iter().map(|o| o.flip).collect::<Vec<_>>(), [true]);
    h.app.dialog = None;
    let mut d = dialog_on(&mut h.app, &lines);
    member(&mut d).corner = corner_type("none");
    d.active = 2;
    d.pick(&h.app.session, x);
    assert!(d.inputs[2].items.is_empty());
    let e = d.error.clone().unwrap_or_default();
    assert!(e.contains("exactly two members end"), "{e}");
}

/// Mitre, Butt and Coped frames split crossings as Auto does: on a Mitre frame a sketch point where
/// the diagonals cross is taken as an override, saved by the lines crossing there with its Flip, its
/// row reads Crossing with an enabled Flip, and Flip keeps the other diagonal whole.
#[test]
fn a_crossing_point_on_a_mitre_frame_is_a_crossing_override() {
    let mut h = tall_harness();
    for (id, p) in [
        ("sketch.create", json!({"plane": "XY", "name": "Frame"})),
        ("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [600, 400]})),
        ("sketch.line", json!({"points": [[0, 0], [600, 400]], "ids": ["d1"]})),
        ("sketch.line", json!({"points": [[0, 400], [600, 0]], "ids": ["d2"]})),
        ("sketch.point", json!({"point": [300, 200], "id": "x"})),
        ("sketch.finish", json!({})),
    ] {
        h.app.run(id, p).unwrap();
    }
    let frame = sketch_id(&h.app, "Frame");
    let lines = frame_lines(&h.app);
    let x = Sel::ExternalSketchPoint { sketch: frame, id: "x".into() };
    let mut d = dialog_on(&mut h.app, &lines);
    member(&mut d).corner = corner_type("mitre");
    d.active = 2;
    d.pick(&h.app.session, x.clone());
    assert_eq!(d.inputs[2].items, std::slice::from_ref(&x), "{:?}", d.error);
    h.app.dialog = Some(d);
    h.frames(2);
    assert!(h.widget_near("Crossing", "Corner 1").is_some(), "the row reads Crossing");
    let [_, y] = h.widget("Corner 1").unwrap();
    let tree = h.output.accesskit_update.as_ref().unwrap();
    let on_row: Vec<_> = tree
        .nodes
        .iter()
        .filter(|(_, n)| n.bounds().is_some_and(|b| b.x1 > b.x0 && (f64::midpoint(b.y0, b.y1) - y).abs() < 2.0))
        .map(|(_, n)| n.clone())
        .collect();
    assert!(on_row.iter().any(|n| n.label() == Some("Flip") && !n.is_disabled()), "an enabled Flip on the row");
    member(h.app.dialog.as_mut().unwrap()).overrides[0].flip = true;
    let c = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    assert_eq!(c[0].1["corners"], json!([{"members": ["d1", "d2"], "kind": "mitre", "flip": true}]), "{}", c[0].1);
    run_all(&mut h.app, c);
    let f = h.app.session.doc.features.last().unwrap();
    let ids = frame::member_line_ids(&h.app.session.model.state(), &f.kind).unwrap();
    assert!(ids.iter().any(|i| i == "d1>d2") && !ids.iter().any(|i| i == "d2>d1"), "{ids:?}");
    // An edit brings the override back as the picked point, flipped.
    let f = f.id;
    let mut e = edit_dialog(&h.app, f, None).unwrap();
    assert_eq!(e.inputs[2].items, std::slice::from_ref(&x), "{:?}", e.extra);
    assert_eq!(member(&mut e).overrides.iter().map(|o| o.flip).collect::<Vec<_>>(), [true]);
}

/// A crossing where another tube also ends (the two diagonals, and a post from the bottom ending
/// where they cross) is a crossing too: the pick is taken and saved by the two diagonals alone, its
/// row reads Crossing with Flip, and Flip changes which diagonal stays whole.
#[test]
fn a_crossing_where_a_tube_also_ends_is_a_crossing_override() {
    let mut h = tall_harness();
    for (id, p) in [
        ("sketch.create", json!({"plane": "XY", "name": "Frame"})),
        ("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [600, 400]})),
        ("sketch.line", json!({"points": [[0, 0], [600, 400]], "ids": ["d1"]})),
        ("sketch.line", json!({"points": [[0, 400], [600, 0]], "ids": ["d2"]})),
        ("sketch.line", json!({"points": [[300, 0], [300, 200]], "ids": ["post"]})),
        ("sketch.finish", json!({})),
    ] {
        h.app.run(id, p).unwrap();
    }
    let frame = sketch_id(&h.app, "Frame");
    let lines = frame_lines(&h.app);
    let x = Sel::ExternalSketchPoint { sketch: frame, id: point_at(&h.app, 300.0, 200.0) };
    let mut d = dialog_on(&mut h.app, &lines);
    member(&mut d).corner = corner_type("auto");
    d.active = 2;
    d.pick(&h.app.session, x.clone());
    assert_eq!(d.inputs[2].items, std::slice::from_ref(&x), "{:?}", d.error);
    let c = apply_commands(&h.app, &d).unwrap();
    assert_eq!(c[0].1["corners"], json!([{"members": ["d1", "d2"], "kind": "auto", "flip": false}]), "{}", c[0].1);
    h.app.dialog = Some(d);
    h.frames(2);
    assert!(h.widget_near("Crossing", "Corner 1").is_some(), "the row reads Crossing");
    // Which diagonal is cut: its later piece is `<it>>‹the whole one›`.
    let cut = |app: &SolveApp| {
        let f = app.session.doc.features.last().unwrap();
        let ids = frame::member_line_ids(&app.session.model.state(), &f.kind).unwrap();
        ["d1>d2", "d2>d1"].into_iter().filter(|p| ids.iter().any(|i| i == p)).collect::<Vec<_>>()
    };
    let mut flipped = h.app.dialog.clone().unwrap();
    run_all(&mut h.app, c);
    let plain = cut(&h.app);
    assert_eq!(plain.len(), 1, "one diagonal is cut: {plain:?}");
    member(&mut flipped).overrides[0].flip = true;
    let c = apply_commands(&h.app, &flipped).unwrap();
    assert_eq!(c[0].1["corners"][0]["flip"], json!(true), "{}", c[0].1);
    h.app.run("edit.undo", json!({})).unwrap();
    run_all(&mut h.app, c);
    let other = cut(&h.app);
    assert_eq!(other.len(), 1, "{other:?}");
    assert_ne!(plain, other, "Flip keeps the other diagonal whole");
}

#[test]
fn switching_groups_keeps_picks_and_placement_through_an_edit() {
    let mut app = frame_app();
    let lines = frame_lines(&app);
    let mut d = dialog_on(&mut app, &lines[..1]);
    let m = member(&mut d);
    m.all = true;
    m.name = "Rail".into();
    m.angle = "15 deg".into();
    if let Kind::Frame(m) = &mut d.kind {
        m.add_group(&mut d.inputs);
        assert!(!m.all);
        assert!(d.inputs[0].items.is_empty());
        m.name = "Post".into();
        m.angle = "90 deg".into();
        m.mirror = true;
        d.inputs[0].items = vec![Sel::ExternalSketchCurve { sketch: sketch_id(&app, "Frame"), id: lines[1].clone() }];
        m.show(0, &mut d.inputs);
        assert_eq!((m.name.as_str(), m.angle.as_str(), m.mirror), ("Rail", "15 deg", false));
        assert_eq!(d.inputs[0].items.len(), 1);
        m.show(1, &mut d.inputs);
        assert_eq!((m.name.as_str(), m.angle.as_str(), m.mirror), ("Post", "90 deg", true));
    }
    let c = apply_commands(&app, &d).unwrap();
    let original = c[0].1.clone();
    assert_eq!(original["groups"][0]["lines"], json!([lines[0]]));
    assert_eq!(original["groups"][1]["lines"], json!([lines[1]]));
    run_all(&mut app, c);
    assert_eq!(body_names(&app), ["Rail", "Post"]);
    let feature = app.session.doc.features.last().unwrap().id;
    let e = edit_dialog(&app, feature, None).unwrap();
    let c = apply_commands(&app, &e).unwrap();
    assert_eq!(c[0].1["params"]["groups"], original["groups"]);
    run_all(&mut app, c);
    assert_eq!(body_names(&app), ["Rail", "Post"]);
}

#[test]
fn group_buttons_stay_visible_and_new_group_is_clickable_at_default_dialog_width() {
    use crate::scenario::Harness;
    use solidcraft_ui_registry::{handle_at, rect_of};

    for (width, height) in [(1280, 720), (800, 600)] {
        for count in [1, 3] {
            let mut h = Harness::new();
            h.app = frame_app();
            h.call("ui.resize", json!({"width": width, "height": height}));
            let lines = frame_lines(&h.app);
            let mut d = dialog_on(&mut h.app, &lines[..1]);
            if let Kind::Frame(m) = &mut d.kind {
                for _ in 1..count {
                    m.add_group(&mut d.inputs);
                }
                m.show(0, &mut d.inputs);
            }
            h.app.dialog = Some(d);
            h.frames(3);

            let visible = rect_of("dialog:card").unwrap().intersect(h.ctx.content_rect());
            let row = rect_of("dialog:row:Groups").unwrap();
            if count == 1 {
                assert!(rect_of("dialog:row:All Lines").unwrap().bottom() <= row.top());
            }
            for label in (1..=count).map(|k| format!("Group {k}")).chain(["New Group".into()]) {
                let key = format!("dialog:button:Groups:{label}");
                let rect = rect_of(&key).unwrap();
                assert!(
                    visible.contains_rect(rect) && row.contains_rect(rect),
                    "{width}x{height}, {count} groups: {key} {rect:?}, row {row:?}, visible {visible:?}"
                );
                assert!(visible.contains(handle_at(&key).unwrap()));
            }
            for k in 1..=count {
                crate::test_support::click(&mut h, &format!("dialog:button:Groups:Group {k}"));
                assert_eq!(member(h.app.dialog.as_mut().unwrap()).current, k - 1);
            }
            crate::test_support::click(&mut h, "dialog:button:Groups:New Group");
            let d = h.app.dialog.as_mut().unwrap();
            assert_eq!(member(d).groups.len(), count + 1, "{width}x{height}, {count} groups: New Group must respond to a click");
            assert_eq!(member(d).current, count);
            assert!(d.inputs[0].items.is_empty());
        }
    }
}

#[test]
fn removing_the_shown_group_keeps_its_neighbours_values() {
    let mut app = frame_app();
    let lines = frame_lines(&app);
    let mut d = dialog_on(&mut app, &lines[..1]);
    member(&mut d).name = "Removed".into();
    if let Kind::Frame(m) = &mut d.kind {
        m.add_group(&mut d.inputs);
        m.name = "Kept".into();
        m.angle = "45 deg".into();
        d.inputs[0].items = vec![Sel::ExternalSketchCurve { sketch: sketch_id(&app, "Frame"), id: lines[1].clone() }];
        m.show(0, &mut d.inputs);
        m.remove_group(&mut d.inputs);
        assert_eq!(m.groups.len(), 1);
        assert_eq!((m.name.as_str(), m.angle.as_str()), ("Kept", "45 deg"));
        m.remove_group(&mut d.inputs);
        assert_eq!(m.groups.len(), 1, "the last group cannot be removed");
    }
    let c = apply_commands(&app, &d).unwrap();
    assert!(c[0].1.get("groups").is_none());
    assert_eq!(c[0].1["lines"], json!([lines[1]]));
    assert_eq!(c[0].1["body_name"], "Kept");
    run_all(&mut app, c);
    assert_eq!(body_names(&app), ["Kept"]);
}

#[test]
fn adding_groups_refuses_joined_corners_without_resetting_flat_frame_controls() {
    let mut app = frame_app();
    let lines = frame_lines(&app);
    let mut d = dialog_on(&mut app, &lines[..1]);
    let Kind::Frame(m) = &mut d.kind else { panic!("not a frame member") };
    m.corner = corner_type("auto");
    m.coped = true;
    m.gap = "2 mm".into();
    m.add_group(&mut d.inputs);
    let err = apply_commands(&app, &d).unwrap_err();
    assert!(err.contains("not supported yet: frame member groups with joined corners"), "{err}");
    let Kind::Frame(m) = &mut d.kind else { panic!("not a frame member") };
    assert_eq!(m.corner, corner_type("auto"));
    assert!(m.coped);
    assert_eq!(m.gap, "2 mm");
    m.remove_group(&mut d.inputs);
    let commands = apply_commands(&app, &d).unwrap();
    let p = &commands[0].1;
    assert!(p.get("groups").is_none());
    assert_eq!(p["corner"], "auto");
    assert_eq!(p["coped"], true);
    assert_eq!(p["gap"], "2 mm");
    assert_eq!(p["fit_ends"], true);
}

/// Preselect two exact support faces as a user would before starting the dialog.
fn gusset_with_picked_supports() -> SolveApp {
    let mut app = app_with(&[
        ("solid.box", json!({"corner":[-20,-20,0],"length":40,"width":40,"height":300,"body_name":"Post"})),
        ("solid.box", json!({"corner":[20,-20,0],"length":300,"width":40,"height":40,"body_name":"Rail"})),
    ]);
    for (name, point, normal) in [("Post", [20., 0., 100.], [1., 0., 0.]), ("Rail", [100., 0., 40.], [0., 0., 1.])] {
        let st = app.session.model.state();
        let body = st.body(name).unwrap();
        let point = solidcore_record::Vec3::new(point[0], point[1], point[2]);
        let normal = solidcore_record::Vec3::new(normal[0], normal[1], normal[2]);
        let index = (0..body.body.face_count())
            .find(|i| body.body.planar_face(*i).is_some_and(|p| p.normal().dot(normal) > 0.999 && (p.origin - point).dot(normal).abs() < 1e-6))
            .unwrap();
        app.session.selection.push(Sel::Face { body: name.into(), index, point });
    }
    app
}

#[test]
/// Preselection, preview and Edit Feature preserve the picks and rebuild the changed plate.
fn gusset_preselection_preview_and_edit_keep_exact_supports() {
    let mut app = gusset_with_picked_supports();
    app.start("frame.gusset");
    let mut d = app.dialog.clone().unwrap();
    assert!(d.previews());
    assert_eq!(d.inputs.iter().map(|i| i.items.len()).collect::<Vec<_>>(), [1, 1]);
    let Kind::Gusset(g) = &mut d.kind else { panic!("wrong dialog") };
    g.leg1 = "80 mm".into();
    g.leg2 = "100 mm".into();
    let preview = crate::dialogs::preview_commands(&app, &d).unwrap();
    assert_eq!(preview[0].0, "frame.gusset");
    assert!(preview[0].1["supports"][0]["index"].is_number());
    let c = apply_commands(&app, &d).unwrap();
    run_all(&mut app, c);
    let fid = app.session.doc.features.last().unwrap().id;
    let mut edit = edit_dialog(&app, fid, None).unwrap();
    assert!(edit.inputs.iter().all(|i| i.items.len() == 1));
    let Kind::Gusset(g) = &mut edit.kind else { panic!("wrong edit dialog") };
    assert_eq!(g.leg2, "100 mm");
    g.thickness = "8 mm".into();
    let c = apply_commands(&app, &edit).unwrap();
    assert_eq!(c[0].0, "timeline.redefine");
    run_all(&mut app, c);
    let state = app.session.model.state();
    let body = state.bodies.iter().find(|b| b.feature == fid).unwrap();
    assert!((body.mesh().measure().volume - 32000.).abs() < 1e-5);
}

/// The outline follows the evaluated clip, and editing thickness preserves its expression.
#[test]
fn gusset_edit_restores_the_outline_from_the_clip_expression() {
    let mut app = gusset_with_picked_supports();
    app.session.execute("parameters.add", &json!({"name":"tip","expression":"0.0 mm"})).unwrap();
    app.start("frame.gusset");
    let mut d = app.dialog.clone().unwrap();
    let Kind::Gusset(g) = &mut d.kind else { panic!("wrong dialog") };
    g.clip = "tip".into();
    g.profile = 1;
    let c = apply_commands(&app, &d).unwrap();
    run_all(&mut app, c);
    let fid = app.session.doc.features.last().unwrap().id;

    let mut edit = edit_dialog(&app, fid, None).unwrap();
    let Kind::Gusset(g) = &mut edit.kind else { panic!("wrong edit dialog") };
    assert_eq!(g.profile, 0);
    g.thickness = "8 mm".into();
    let c = apply_commands(&app, &edit).unwrap();
    assert_eq!(c[0].1["params"]["clip"], "tip");
    run_all(&mut app, c);
    app.session.execute("parameters.change", &json!({"name":"tip","expression":"10 mm"})).unwrap();
    let edit = edit_dialog(&app, fid, None).unwrap();
    let Kind::Gusset(g) = &edit.kind else { panic!("wrong edit dialog") };
    assert_eq!(g.profile, 1);
}

#[test]
/// Click each icon and verify both the resulting plate bounds and translated accessible names.
fn gusset_placement_icons_select_both_sides_and_center_with_exact_bounds() {
    let mut h = crate::scenario::Harness::new();
    h.app = gusset_with_picked_supports();
    h.app.home.open = false;
    h.app.start("frame.gusset");
    h.frames(3);
    assert!(crate::scenario::rect_of("dialog:segments:Thickness Placement").is_some(), "placement is a visible icon strip");
    let nodes = &h.output.accesskit_update.as_ref().unwrap().nodes;
    for label in ["Negative Side", "Centered", "Positive Side"] {
        assert!(nodes.iter().any(|(_, n)| n.role() == egui::accesskit::Role::Button && n.label() == Some(label)), "accessible icon: {label}");
    }
    for (label, placement, min, max) in
        [("Negative Side", "negative", 0., 6.), ("Centered", "center", -3., 3.), ("Positive Side", "positive", -6., 0.)]
    {
        crate::test_support::click(&mut h, &format!("dialog:Thickness Placement:{label}"));
        let d = h.app.dialog.as_ref().unwrap();
        let commands = crate::dialogs::preview_commands(&h.app, d).unwrap();
        assert_eq!(commands[0].1["placement"], placement);
        let mut preview = Session::new(solidcraft_engine::doc::Document::from_json(&h.app.session.doc.to_json()).unwrap());
        let result = preview.execute(&commands[0].0, &commands[0].1).unwrap();
        let state = preview.model.state();
        let body = state.bodies.iter().find(|b| b.feature == result["feature"].as_u64().unwrap()).unwrap();
        let bounds = body.mesh().bounds();
        assert!((bounds.min.y - min).abs() < 1e-6 && (bounds.max.y - max).abs() < 1e-6, "{label}: {bounds:?}");
    }
    // Icon-only controls still have translated names for keyboard/screen-reader users.
    h.app.preferences.language = "de".into();
    h.frames(3);
    let nodes = &h.output.accesskit_update.as_ref().unwrap().nodes;
    for id in ["ui.gusset.negative", "ui.gusset.center", "ui.gusset.positive"] {
        assert!(nodes.iter().any(|(_, n)| n.role() == egui::accesskit::Role::Button && n.label() == Some(h.app.tr(id))), "{id}");
    }
}

fn plate_app() -> (SolveApp, Sel) {
    let app = app_with(&[
        ("sketch.create", json!({"plane":"XY","name":"Post"})),
        ("sketch.options", json!({"sketch_3d":true})),
        ("sketch.line3d", json!({"points":[[0,0,0],[0,0,300]]})),
        ("sketch.finish", json!({})),
        ("frame.member", json!({"sketch":"Post","profile":{"kind":"rect_tube","width":40,"height":40,"wall":3,"outer_radius":0},"body_name":"Post"})),
    ]);
    let state = app.session.world_state().unwrap();
    let body = state.body("Post").unwrap();
    let face = body.body.faces(0.001).unwrap().into_iter().find(|f| f.plane_normal.is_some_and(|n| n.z > 0.999)).unwrap();
    let mesh = body.mesh();
    let (tri, _) = mesh.triangles.iter().zip(&mesh.tri_face).find(|(_, i)| **i as usize == face.index).unwrap();
    let [a, b, c] = mesh.tri(tri).unwrap();
    let pick = Sel::Face { body: body.name.clone(), index: face.index, point: (a + b + c) / 3. };
    (app, pick)
}

#[test]
fn mounting_plate_dialog_preselects_previews_and_edits_a_bolt_circle() {
    let (mut app, pick) = plate_app();
    app.session.selection = vec![pick.clone()];
    app.start("frame.plate");
    let mut dialog = app.dialog.clone().unwrap();
    assert_eq!(dialog.inputs[0].items, [pick]);
    let Kind::FramePlate(plate) = &mut dialog.kind else { panic!("plate dialog") };
    plate.shape = 1;
    plate.diameter = "120 mm".into();
    plate.pattern = 2;
    plate.count = "6".into();
    plate.pitch_diameter = "90 mm".into();
    plate.pattern_angle = "15 deg".into();
    let commands = apply_commands(&app, &dialog).unwrap();
    let before = app.session.doc.to_json();
    let preview = app.session.preview_visible_world(&commands).unwrap();
    assert_eq!(app.session.doc.to_json(), before, "preview leaves the document alone");
    assert_eq!(preview.after.bodies.len(), 2);
    run_all(&mut app, commands);
    let feature = app.session.doc.features.last().unwrap().id;
    let mut edited = edit_dialog(&app, feature, None).unwrap();
    assert_eq!(edited.inputs[0].items.len(), 1, "support face restored on material, not in the tube's bore");
    let Kind::FramePlate(plate) = &mut edited.kind else { panic!("plate dialog") };
    assert_eq!((plate.shape, plate.pattern, plate.count.as_str()), (1, 2, "6"));
    assert_eq!(plate.pattern_angle, "15 deg");
    plate.thickness = "10 mm".into();
    let commands = apply_commands(&app, &edited).unwrap();
    assert_eq!(commands[0].0, "timeline.redefine");
    run_all(&mut app, commands);
    assert_eq!(app.session.world_state().unwrap().bodies.len(), 2, "editing replaces the plate");
    let FeatureKind::FramePlate { thickness, .. } = &app.session.doc.feature(feature).unwrap().kind else { panic!("plate feature") };
    assert_eq!(thickness.as_str(), "10 mm");
}

#[test]
fn mounting_plate_dialog_requires_a_face_and_whole_hole_count() {
    let (mut app, pick) = plate_app();
    app.start("frame.plate");
    let mut dialog = app.dialog.clone().unwrap();
    assert!(apply_commands(&app, &dialog).is_err());
    dialog.inputs[0].items = vec![pick];
    let Kind::FramePlate(plate) = &mut dialog.kind else { panic!("plate dialog") };
    plate.pattern = 2;
    plate.count = "4.5".into();
    assert!(apply_commands(&app, &dialog).unwrap_err().contains("whole number"));
}
