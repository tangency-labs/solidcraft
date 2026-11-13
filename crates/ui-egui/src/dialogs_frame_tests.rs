use crate::test_support::run_all;
use serde_json::json;
use solidcore_record::Vec3;
use solidcraft_engine::Session;
use solidcraft_engine::geom::prelude::*;

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
    app_with(&[
        ("sketch.create", json!({"plane": "XY", "name": "Frame"})),
        ("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [600, 400]})),
        ("sketch.finish", json!({})),
        ("sketch.create", json!({"plane": "YZ", "name": "Ring"})),
        ("sketch.circle.center", json!({"center": [0, 0], "radius": 20})),
        ("sketch.circle.center", json!({"center": [0, 0], "radius": 17})),
        ("sketch.finish", json!({})),
    ])
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
fn a_lone_last_point_waits_with_pick_another_point() {
    let mut app = box_app();
    let mut d = dialog_on(&mut app, &[]);
    pick(&app, &mut d, corner(0., 0., 0.));
    assert_eq!(d.error.as_deref(), Some("Pick another point"));
    pick_all(&app, &mut d, &[corner(0., 0., 400.), corner(600., 0., 0.)]);
    assert_eq!(d.error.as_deref(), Some("Pick another point"));
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
fn a_lone_point_before_a_line_is_refused() {
    let mut app = box_app();
    let base = sketch_id(&app, "Base");
    let line = Sel::ExternalSketchCurve { sketch: base, id: app.session.doc.sketch(base).unwrap().curves[0].id.clone() };
    let mut d = dialog_on(&mut app, &[]);
    pick(&app, &mut d, corner(0., 0., 0.));
    pick(&app, &mut d, line.clone());
    assert_eq!(path(&d), [corner(0., 0., 0.)], "the line is taken back");
    let e = d.error.clone().unwrap();
    assert!(e.starts_with("Can't add that pick:") && e.contains("A point needs another point after it"), "{e}");
    // Lines first, or after a whole pair, are fine.
    d.inputs[0].items = vec![corner(0., 0., 0.), line.clone()];
    assert_eq!(apply_commands(&app, &d).unwrap_err(), "A point needs another point after it");
    d.inputs[0].items = vec![line.clone(), corner(0., 0., 0.), corner(0., 0., 400.)];
    let c = apply_commands(&app, &d).unwrap();
    assert_eq!((&c[0].1["sketch"], c[0].1["pairs"].as_array().map(Vec::len)), (&json!(base), Some(1)));
}

#[test]
fn the_same_point_twice_in_a_row_is_refused() {
    let mut app = box_app();
    let mut d = dialog_on(&mut app, &[]);
    pick_all(&app, &mut d, &[corner(0., 0., 0.), corner(0., 0., 0.)]);
    assert_eq!(path(&d), [corner(0., 0., 0.)], "the pick is undone");
    let e = d.error.clone().unwrap();
    assert!(e.starts_with("Can't add that pick:"), "{e}");
    assert!(crate::dialogs_frame::refused_pick(&e), "shown as guidance, with no Create Issue");
    // The next good pick pairs with the waiting point.
    pick(&app, &mut d, corner(0., 0., 400.));
    assert_eq!((path(&d).len(), d.error.as_deref()), (2, None));
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
    assert_eq!(d.error.as_deref(), Some("Pick another point"), "p2 now waits");
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
    crate::viewport::Proj { cam: app.cam, vp: app.cam.view_proj(960.0 / 600.0, crate::viewport::scene_radius(app)), rect }
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

/// p0, p1, p1, p2: the path's picked list shows the shared corner once for each pair it is in,
/// and a row's remove button takes out that occurrence only, so the other pair keeps its point,
/// its reference and its id.
#[test]
fn the_picked_list_shows_each_occurrence_and_a_rows_remove_takes_out_only_that_one() {
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
    assert_eq!(path(h.app.dialog.as_ref().unwrap()), [p0.clone(), p1.clone(), p1.clone(), p2.clone()]);
    let m = member(h.app.dialog.as_mut().unwrap());
    let (refs, ids) = (m.refs.clone(), m.pair_ids.clone());
    assert_eq!(ids[1].as_deref(), Some("pair1"));

    h.step(&json!({"click": {"handle": "dialog:list_toggle:Path"}})).unwrap();
    h.frames(2);
    let rows = (0..8).take_while(|i| crate::scenario::handle_at(&format!("dialog:list:Path:{i}:remove")).is_some()).count();
    assert_eq!(rows, 4, "a row for each occurrence");
    // The second p1 is the second pair's.
    h.step(&json!({"click": {"handle": "dialog:list:Path:2:remove"}})).unwrap();
    h.frames(2);
    let d = h.app.dialog.as_mut().unwrap();
    assert_eq!(path(d), [p0, p1, p2], "only that occurrence goes");
    let m = member(d);
    assert_eq!(m.refs, [refs[0].clone(), refs[1].clone(), refs[3].clone()]);
    assert_eq!(m.pair_ids, [ids[0].clone(), ids[1].clone(), ids[3].clone()]);
    assert_eq!(d.error.as_deref(), Some("Pick another point"), "p2 now waits");
    let c = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    assert_eq!(c[0].1["params"]["pairs"], json!([stored[0]]), "the first pair is intact, id and references");
}
