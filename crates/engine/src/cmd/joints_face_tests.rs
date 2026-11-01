//! Joint origins found by their face's name: a part jointed to a hole stays on it, on the same
//! side and turned the same way, as the design's parameters change.

use serde_json::{Value, json};
use solidcraft_doc::joints::{Snap, XRef};
use solidcraft_geom::Vec3;

use super::tests::{occ, place, run};
use crate::Session;

fn near(a: Vec3, b: Vec3) -> bool {
    a.dist(b) < 1e-6
}

#[test]
fn joint_origin_names_in_face_or_circle_explain_the_correct_form() {
    let mut s = Session::default();
    for (name, corner) in [("A", [0, 0, 0]), ("B", [40, 0, 0])] {
        run(&mut s, "component.create", json!({"name": name}));
        run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 10, "corner": corner}));
        run(&mut s, "component.activate", json!({"component": "root"}));
    }
    run(&mut s, "joint.origin", json!({"name": "TopA", "occurrence": "A:1", "face": [10, 10, 10]}));
    run(&mut s, "joint.origin", json!({"name": "BotB", "occurrence": "B:1", "face": [50, 10, 0]}));
    for field in ["face", "circle"] {
        for (side, name, occurrence) in [("a", "TopA", "A:1"), ("b", "BotB", "B:1")] {
            let mut params = json!({"type": "rigid", "a": "TopA", "b": "BotB"});
            params[side] = json!({"occurrence": occurrence, field: name});
            let error = s.execute("joint.create", &params).unwrap_err().to_string();
            assert_eq!(
                error,
                format!("invalid parameters for `joint.create`: `{name}` is a joint origin: pass it as the whole origin, not as a `{field}` name")
            );
            assert!(s.doc.assembly.joints.is_empty());
        }
    }
    let error = s.execute("joint.create", &json!({"type": "rigid", "a": {"face": "Missing"}, "b": "BotB"})).unwrap_err().to_string();
    assert!(error.contains("no face is named `Missing`"), "{error}");
    let result = run(&mut s, "joint.create", json!({"type": "rigid", "a": "TopA", "b": "BotB"}));
    assert_eq!(result["errors"].as_array().map(Vec::len), Some(0), "{result}");
    assert_eq!(s.doc.assembly.joints.len(), 1);
}

/// Widths swept: small steps (a hole's wall in a 1 mm plate is a few long facets, and a pick
/// stored at the old width lies beside the moved hole) and large ones.
const WIDTHS: [f64; 9] = [302.0, 304.0, 310.0, 330.0, 400.0, 450.0, 510.0, 301.0, 300.0];

/// A tray: a 1 mm plate `W` wide (a parameter, 300 mm to start) and 100 deep, with a 6 mm hole
/// 20 mm in from its right edge (an expression); grounded. A bracket: a 20 x 20 x 2 foot with a
/// 6 mm hole in its middle and a 15 mm upright along its +X edge, so a turn shows.
fn tray() -> Session {
    let mut s = Session::default();
    run(&mut s, "parameters.change", json!({"name": "W", "expression": "300 mm"}));
    run(&mut s, "component.create", json!({"name": "Tray"}));
    run(&mut s, "solid.box", json!({"length": "W", "width": 100, "height": 1, "corner": [0, 0, 0]}));
    run(&mut s, "solid.hole", json!({"position": ["W - 20 mm", 50, 1], "diameter": 6}));
    let tray = occ(&s, "Tray");
    run(&mut s, "occurrence.ground", json!({"occurrence": tray, "grounded": true}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "component.create", json!({"name": "Bracket"}));
    run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 2, "corner": [100, 200, 0]}));
    run(&mut s, "solid.box", json!({"length": 2, "width": 20, "height": 15, "corner": [118, 200, 0], "operation": "join"}));
    run(&mut s, "solid.hole", json!({"position": [110, 210, 2], "diameter": 6}));
    s
}

fn width(s: &mut Session, w: f64) {
    run(s, "parameters.change", json!({"name": "W", "expression": format!("{w} mm")}));
    let r = run(s, "joint.solve", json!({}));
    assert_eq!(r["errors"].as_array().map(Vec::len), Some(0), "W = {w}: {r}");
}

/// The bracket sits on the tray's top, its hole on the tray's hole and its upright toward +X.
fn on_the_hole(s: &Session, w: f64) {
    let hole = place(s, "Bracket", Vec3::new(110.0, 210.0, 0.0));
    assert!(near(hole, Vec3::new(w - 20.0, 50.0, 1.0)), "W = {w}: the bracket's hole is at {hole:?}");
    let upright = place(s, "Bracket", Vec3::new(119.0, 210.0, 15.0));
    assert!(near(upright, Vec3::new(w - 11.0, 50.0, 16.0)), "W = {w}: the upright's top is at {upright:?}");
}

fn origin(s: &Session, joint: &str, a: bool) -> Snap {
    let j = s.doc.assembly.joints.iter().find(|j| j.name == joint).map(|j| if a { &j.a } else { &j.b });
    j.map(|o| o.snap.clone()).unwrap_or(Snap::Point { point: Vec3::ZERO, z: None, x: None })
}

#[test]
fn a_bracket_on_a_hole_in_thin_plate_stays_on_it_as_the_plate_widens() {
    let mut s = tray();
    // Picks inside the 1 mm wall of the tray's hole (nearer its top) and the 2 mm wall of the
    // bracket's (nearer its bottom); both x along +X.
    let r = run(
        &mut s,
        "joint.create",
        json!({"type": "rigid", "name": "Mount",
               "a": {"occurrence": "Tray", "circle": [283, 50, 0.8], "x": [1, 0, 0]},
               "b": {"occurrence": "Bracket", "circle": [113, 210, 0.4], "x": [1, 0, 0]}}),
    );
    assert_eq!(r["errors"].as_array().map(Vec::len), Some(0), "{r}");
    on_the_hole(&s, 300.0);
    // Both origins are kept by their wall's name and their end's (the tray's top face, the
    // bracket's bottom one).
    for a in [true, false] {
        let snap = origin(&s, "Mount", a);
        assert!(matches!(&snap, Snap::CircleCenter { face: Some(_), end: Some(_), x: Some(XRef::Dir(_)), .. }), "{snap:?}");
    }
    for w in WIDTHS {
        width(&mut s, w);
        on_the_hole(&s, w);
    }
    // Saved and opened again, it still follows.
    let dir = std::env::temp_dir().join(format!("solidcraft-joint-faces-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("tray.json").to_string_lossy().to_string();
    run(&mut s, "file.save", json!({"path": path}));
    run(&mut s, "file.new", json!({}));
    run(&mut s, "doc.open", json!({"path": path}));
    for w in [330.0, 510.0] {
        width(&mut s, w);
        on_the_hole(&s, w);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_circle_given_by_its_centre_follows_its_hole() {
    let mut s = tray();
    // The centres of the tray hole's top rim and the bracket hole's bottom rim.
    let r = run(
        &mut s,
        "joint.create",
        json!({"type": "rigid", "name": "Mount",
               "a": {"occurrence": "Tray", "circle": [280, 50, 1], "x": [1, 0, 0]},
               "b": {"occurrence": "Bracket", "circle": [110, 210, 0], "x": [1, 0, 0]}}),
    );
    assert_eq!(r["errors"].as_array().map(Vec::len), Some(0), "{r}");
    let snap = origin(&s, "Mount", true);
    assert!(matches!(&snap, Snap::CircleCenter { face: Some(_), end: Some(_), .. }), "a centre names its face and end: {snap:?}");
    on_the_hole(&s, 300.0);
    for w in WIDTHS {
        width(&mut s, w);
        on_the_hole(&s, w);
    }
}

/// The name `model.faces` gives the face of `component`'s body holding `p` (component coordinates).
fn face_name(s: &mut Session, component: &str, p: Vec3) -> String {
    let st = s.model.state();
    let c = s.doc.find_component(component).unwrap_or(0);
    solidcraft_doc::joints::face_name_at(&s.doc, &st, c, p, false).unwrap_or_default()
}

#[test]
fn origins_take_faces_by_name_and_an_end_and_turn_by_name() {
    let mut s = tray();
    let wall = face_name(&mut s, "Tray", Vec3::new(283.0, 50.0, 0.5));
    let top = face_name(&mut s, "Tray", Vec3::new(150.0, 50.0, 1.0));
    let bottom = face_name(&mut s, "Tray", Vec3::new(150.0, 50.0, 0.0));
    let right = face_name(&mut s, "Tray", Vec3::new(300.0, 50.0, 0.5));
    let foot = face_name(&mut s, "Bracket", Vec3::new(113.0, 210.0, 1.0));
    let under = face_name(&mut s, "Bracket", Vec3::new(105.0, 205.0, 0.0));
    assert!(![&wall, &top, &bottom, &right, &foot, &under].iter().any(|n| n.is_empty()));
    // A round end by the wall's name and the face it opens onto; x along the tray's right end
    // face's normal (+X).
    run(&mut s, "joint.origin", json!({"name": "Hole", "circle": wall, "end": top, "x": right}));
    let r = run(&mut s, "joint.create", json!({"type": "rigid", "name": "Mount", "a": "Hole", "b": {"circle": foot, "end": under, "x": [1, 0, 0]}}));
    assert_eq!(r["errors"].as_array().map(Vec::len), Some(0), "{r}");
    on_the_hole(&s, 300.0);
    for w in [304.0, 400.0, 510.0] {
        width(&mut s, w);
        on_the_hole(&s, w);
    }
    // The other end: the bracket hangs under the plate (its foot's underside on the plate's).
    run(&mut s, "joint.origin", json!({"name": "Under", "circle": wall, "end": bottom, "x": right}));
    run(&mut s, "joint.delete", json!({"joint": "Mount"}));
    run(&mut s, "joint.create", json!({"type": "rigid", "name": "Hung", "a": "Under", "b": {"circle": foot, "end": under, "x": [1, 0, 0]}}));
    let hole = place(&s, "Bracket", Vec3::new(110.0, 210.0, 0.0));
    assert!(near(hole, Vec3::new(490.0, 50.0, 0.0)), "{hole:?}");
    assert!(place(&s, "Bracket", Vec3::new(119.0, 210.0, 15.0)).z < -10.0);
    // An end the hole doesn't have, a missing face and a bad x are refused.
    for bad in [
        json!({"circle": wall, "end": right}),
        json!({"circle": "F999:nothing"}),
        json!({"circle": wall, "x": "F999:nothing"}),
        json!({"circle": wall, "x": [0, 0, 0]}),
        json!({"face": [10, 10, 1], "end": top}),
    ] {
        assert!(s.execute("joint.origin", &bad).is_err(), "{bad}");
    }
}

/// A face split by a cut whose edge runs in many segments is named after the faces around its
/// piece (`<face>#[<face>,…]`): several hundred characters. Such a name, as `model.faces` gives
/// it, is taken for a face centre and as a round end (it was refused over 512 characters).
#[test]
fn origins_take_a_long_piece_name() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Tray"}));
    run(&mut s, "solid.box", json!({"length": 300, "width": 100, "height": 1, "corner": [0, 0, 0]}));
    // A slot across the tray whose left edge zigzags in 60 segments: the top left of it names them all.
    let mut zig: Vec<[f64; 2]> = (0..=60).map(|i| [140.0 + f64::from(i % 2) * 4.0, -10.0 + 2.0 * f64::from(i)]).collect();
    zig.extend([[170.0, 110.0], [170.0, -10.0]]);
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Zig"}));
    run(&mut s, "sketch.line", json!({"points": zig, "closed": true}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "Zig", "through_all": true, "direction": "symmetric", "operation": "cut"}));
    run(&mut s, "solid.hole", json!({"position": [50, 50, 1], "diameter": 6}));
    let tray = occ(&s, "Tray");
    run(&mut s, "occurrence.ground", json!({"occurrence": tray, "grounded": true}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "component.create", json!({"name": "Bracket"}));
    run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 2, "corner": [100, 200, 0]}));
    run(&mut s, "solid.hole", json!({"position": [110, 210, 2], "diameter": 6}));
    let top = face_name(&mut s, "Tray", Vec3::new(20.0, 20.0, 1.0));
    // Split-face names are bounded at 512 bytes (#5860), so the name is as long as a piece name gets;
    // joints take far more than that (`MAX_FACE_NAME`), so none is refused for its length.
    assert!(top.len() > 400 && top.len() <= 512, "{}: {top}", top.len());
    let wall = face_name(&mut s, "Tray", Vec3::new(53.0, 50.0, 0.5));
    let foot = face_name(&mut s, "Bracket", Vec3::new(113.0, 210.0, 1.0));
    let under = face_name(&mut s, "Bracket", Vec3::new(105.0, 205.0, 0.0));
    run(&mut s, "joint.origin", json!({"name": "Top", "face": top}));
    run(&mut s, "joint.origin", json!({"name": "Hole", "circle": wall, "end": top, "x": [1, 0, 0]}));
    let r = run(&mut s, "joint.create", json!({"type": "rigid", "a": "Hole", "b": {"circle": foot, "end": under, "x": [1, 0, 0]}}));
    assert_eq!(r["errors"].as_array().map(Vec::len), Some(0), "{r}");
    let hole = place(&s, "Bracket", Vec3::new(110.0, 210.0, 0.0));
    assert!(near(hole, Vec3::new(50.0, 50.0, 1.0)), "{hole:?}");
    // Names stay bounded.
    let huge = format!("F1:+z#[{}]", "F2:side:l1,".repeat(2_000));
    assert!(s.execute("joint.origin", &json!({"name": "Huge", "face": huge})).is_err());
}

#[test]
fn a_face_centre_by_name_follows_its_face() {
    let mut s = tray();
    let right = face_name(&mut s, "Tray", Vec3::new(300.0, 50.0, 0.5));
    run(&mut s, "joint.origin", json!({"name": "End", "face": right, "x": [0, 1, 0]}));
    let frame = |s: &Session| {
        let o = s.doc.assembly.origins.iter().find(|o| o.name == "End").map(|o| o.origin.clone());
        o.and_then(|o| solidcraft_doc::joints::resolve_origin(&s.doc, &s.model.state(), &o).ok())
    };
    for w in [300.0, 330.0, 510.0] {
        width(&mut s, w);
        let m = frame(&s).unwrap_or(solidcraft_doc::IDENTITY);
        let (o, x, z) = (Vec3::new(m[3][0], m[3][1], m[3][2]), Vec3::new(m[0][0], m[0][1], m[0][2]), Vec3::new(m[2][0], m[2][1], m[2][2]));
        assert!(near(o, Vec3::new(w, 50.0, 0.5)) && near(z, Vec3::X) && near(x, Vec3::Y), "W = {w}: {o:?} {x:?} {z:?}");
    }
}

#[test]
fn an_origin_without_an_end_or_turn_opens_as_before() {
    // A design saved before origins named their end and turn: the snap reads without them.
    let snap: Snap = serde_json::from_value(json!({"type": "circle_center", "pick": [1, 2, 3], "face": "F1:side"})).unwrap_or(Snap::Point {
        point: Vec3::ZERO,
        z: None,
        x: None,
    });
    assert_eq!(snap, Snap::CircleCenter { pick: Vec3::new(1.0, 2.0, 3.0), face: Some("F1:side".into()), end: None, x: None });
    let back: Value = serde_json::to_value(&snap).unwrap_or_default();
    assert_eq!(back, json!({"type": "circle_center", "pick": [1.0, 2.0, 3.0], "face": "F1:side"}));
    let turned = Snap::FaceCenter { pick: Vec3::ZERO, face: None, x: Some(XRef::Face { face: "F2:+x".into(), pick: None }) };
    let v = serde_json::to_value(&turned).unwrap_or_default();
    assert_eq!(v["x"], json!({"face": "F2:+x"}));
    assert_eq!(serde_json::from_value::<Snap>(v).ok(), Some(turned));
}

/// The frame a named joint origin resolves to: its origin and z.
fn origin_frame(s: &Session, name: &str) -> Option<(Vec3, Vec3)> {
    let o = s.doc.assembly.origins.iter().find(|o| o.name == name)?;
    let m = solidcraft_doc::joints::resolve_origin(&s.doc, &s.model.state(), &o.origin).ok()?;
    Some((Vec3::new(m[3][0], m[3][1], m[3][2]), Vec3::new(m[2][0], m[2][1], m[2][2])))
}

#[test]
fn a_round_origin_saved_without_an_end_takes_the_end_it_took_before() {
    // A plate `T` thick (5 mm) with a 6 mm hole through it, and two round origins as builds
    // before `end` saved them (no `end`, no `x`): picks on the hole's wall 4 mm and 1 mm up.
    let mut s = Session::default();
    run(&mut s, "parameters.change", json!({"name": "T", "expression": "5 mm"}));
    run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": "T", "corner": [0, 0, 0]}));
    run(&mut s, "solid.hole", json!({"position": [20, 20, "T"], "diameter": 6}));
    let wall = solidcraft_doc::joints::face_name_at(&s.doc, &s.model.state(), 0, Vec3::new(23.0, 20.0, 2.5), true).unwrap_or_default();
    assert!(!wall.is_empty());
    for (name, z) in [("High", 4.0), ("Low", 1.0)] {
        let saved = json!({"name": name, "origin": {"occurrence": 0, "snap": {"face": wall, "pick": [23.0, 20.0, z], "type": "circle_center"}}});
        let o: solidcraft_doc::joints::NamedOrigin = serde_json::from_value(saved).unwrap_or_else(|e| panic!("{e}"));
        s.doc_mut().assembly.origins.push(o);
    }
    let dir = std::env::temp_dir().join(format!("solidcraft-joint-end-tie-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("plate.json").to_string_lossy().to_string();
    run(&mut s, "file.save", json!({"path": path}));
    run(&mut s, "file.new", json!({}));
    run(&mut s, "doc.open", json!({"path": path}));
    let _ = std::fs::remove_dir_all(&dir);
    let (top, bottom) = (|t: f64| (Vec3::new(20.0, 20.0, t), Vec3::Z), (Vec3::new(20.0, 20.0, 0.0), -Vec3::Z));
    // Where the pick is nearer one end, that end; midway along the wall (the plate 8 mm thick
    // for the pick 4 mm up, 2 mm for the one 1 mm up), the bottom end, as those builds took.
    for (t, high, low) in [(5.0, Some(top(5.0)), bottom), (8.0, Some(bottom), bottom), (2.0, None, bottom), (5.0, Some(top(5.0)), bottom)] {
        run(&mut s, "parameters.change", json!({"name": "T", "expression": format!("{t} mm")}));
        for (name, want) in [("High", high), ("Low", Some(low))] {
            let Some((o, z)) = want else { continue };
            let got = origin_frame(&s, name);
            assert!(got.is_some_and(|(go, gz)| near(go, o) && near(gz, z)), "T = {t}, {name}: {got:?}, expected {o:?} {z:?}");
        }
    }
}

#[test]
fn origins_on_a_body_placed_by_an_offset_agree_named_or_not() {
    // A plate made at the origin, then placed by an offset (turned a quarter about x, moved):
    // a body moved in from another component keeps its geometry where it was made.
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 5, "corner": [0, 0, 0], "body_name": "Plate"}));
    run(&mut s, "solid.hole", json!({"position": [20, 20, 5], "diameter": 6}));
    let offset = solidcraft_doc::rigid(Vec3::new(7.0, 0.0, 3.0), Vec3::ZERO, Vec3::X, std::f64::consts::FRAC_PI_2);
    s.doc_mut().body_offsets.insert("Plate".into(), offset);
    s.refresh();
    let at = |p: Vec3| solidcraft_doc::apply_point(&offset, p);
    let up = solidcraft_doc::apply_vector(&offset, Vec3::Z);
    let st = s.model.state();
    let (top_pick, wall_pick) = (at(Vec3::new(10.0, 10.0, 5.0)), at(Vec3::new(23.0, 20.0, 4.0)));
    let top = solidcraft_doc::joints::face_name_at(&s.doc, &st, 0, top_pick, false);
    let wall = solidcraft_doc::joints::face_name_at(&s.doc, &st, 0, wall_pick, true);
    assert!(top.is_some() && wall.is_some(), "{top:?} {wall:?}");
    let resolve = |snap: Snap| {
        let o = solidcraft_doc::joints::JointOrigin { occurrence: 0, snap };
        let m = solidcraft_doc::joints::resolve_origin(&s.doc, &st, &o).unwrap_or_else(|e| panic!("{e}"));
        (Vec3::new(m[3][0], m[3][1], m[3][2]), Vec3::new(m[2][0], m[2][1], m[2][2]))
    };
    // The top face's centre: by its name, and by the point alone.
    let named = resolve(Snap::FaceCenter { pick: top_pick, face: top, x: None });
    let unnamed = resolve(Snap::FaceCenter { pick: top_pick, face: None, x: None });
    assert!(near(named.0, unnamed.0) && near(named.1, unnamed.1), "{named:?} {unnamed:?}");
    assert!(near(named.1, up) && (named.0 - at(Vec3::new(0.0, 0.0, 5.0))).dot(up).abs() < 1e-6, "{named:?}");
    // The hole's top end (the pick nearer it): by the wall's name, and by the point alone.
    let named = resolve(Snap::CircleCenter { pick: wall_pick, face: wall, end: None, x: None });
    let unnamed = resolve(Snap::CircleCenter { pick: wall_pick, face: None, end: None, x: None });
    assert!(near(named.0, unnamed.0) && near(named.1, unnamed.1), "{named:?} {unnamed:?}");
    assert!(near(named.0, at(Vec3::new(20.0, 20.0, 5.0))) && near(named.1, up), "{named:?}");
}

#[test]
fn a_round_origin_keeps_its_end_when_the_face_it_opens_onto_gets_other_neighbours() {
    // A plate `T` thick with a groove across its top (so the top face is in pieces, each named
    // by the faces round it), a small hole and a 6 mm hole in the left piece. The origin is
    // picked on the 6 mm hole's wall near the top, and names the top piece as its end.
    let mut s = Session::default();
    run(&mut s, "parameters.change", json!({"name": "T", "expression": "2 mm"}));
    run(&mut s, "component.create", json!({"name": "Plate"}));
    run(&mut s, "solid.box", json!({"length": 60, "width": 100, "height": "T", "corner": [0, 0, 0]}));
    run(&mut s, "solid.box", json!({"length": 4, "width": 100, "height": 1, "corner": [28, 0, "T - 1 mm"], "operation": "cut"}));
    run(&mut s, "solid.hole", json!({"position": [10, 50, "T"], "diameter": 4}));
    let small = s.doc.features.last().map_or(0, |f| f.id);
    run(&mut s, "solid.hole", json!({"position": [20, 50, "T"], "diameter": 6}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "joint.origin", json!({"occurrence": "Plate", "circle": [23, 50, 1.6], "name": "O"}));
    let top = |s: &Session| {
        let o = s.doc.assembly.origins.iter().find(|o| o.name == "O").map(|o| o.origin.clone());
        o.and_then(|o| solidcraft_doc::joints::resolve_origin(&s.doc, &s.model.state(), &o).ok()).map(|m| (m[3][2], m[2][2]))
    };
    assert_eq!(top(&s), Some((2.0, 1.0)));
    // Thicker, the pick is nearer the bottom; without the small hole the top piece is named by
    // fewer faces. The origin stays on the top.
    run(&mut s, "parameters.change", json!({"name": "T", "expression": "4 mm"}));
    run(&mut s, "timeline.suppress", json!({"feature": small, "suppressed": true}));
    let (z, dz) = top(&s).unwrap_or_default();
    assert!((z - 4.0).abs() < 1e-9 && (dz - 1.0).abs() < 1e-9, "{z} {dz}");
}

#[test]
fn a_face_centre_stays_put_when_a_hole_in_its_face_moves_or_goes() {
    // A 200 x 100 plate with a 20 mm hole at `HX` in its top face, and an origin at the top
    // face's centre, by the face's name and by a point alone. Moving the hole or suppressing it
    // changes the face's area, not its outline: the origin stays at the outline's middle.
    let mut s = Session::default();
    run(&mut s, "parameters.change", json!({"name": "HX", "expression": "60 mm"}));
    run(&mut s, "component.create", json!({"name": "Plate"}));
    run(&mut s, "solid.box", json!({"length": 200, "width": 100, "height": 2, "corner": [0, 0, 0]}));
    run(&mut s, "solid.hole", json!({"position": ["HX", 50, 2], "diameter": 20, "name": "H"}));
    let pick = Vec3::new(150.0, 80.0, 2.0);
    let top = face_name(&mut s, "Plate", pick);
    run(&mut s, "joint.origin", json!({"name": "Top", "face": top}));
    let by_point = |s: &Session| {
        let o = solidcraft_doc::joints::JointOrigin { occurrence: occ(s, "Plate"), snap: Snap::FaceCenter { pick, face: None, x: None } };
        let m = solidcraft_doc::joints::resolve_origin(&s.doc, &s.model.state(), &o).ok()?;
        Some((Vec3::new(m[3][0], m[3][1], m[3][2]), Vec3::new(m[2][0], m[2][1], m[2][2])))
    };
    let mut seen = vec![(origin_frame(&s, "Top"), by_point(&s))];
    for (cmd, p) in [
        ("parameters.change", json!({"name": "HX", "expression": "80 mm"})),
        ("timeline.suppress", json!({"feature": "H", "suppressed": true})),
        ("parameters.change", json!({"name": "HX", "expression": "40 mm"})),
        ("timeline.suppress", json!({"feature": "H", "suppressed": false})),
    ] {
        run(&mut s, cmd, p);
        seen.push((origin_frame(&s, "Top"), by_point(&s)));
    }
    for (i, f) in seen.iter().enumerate() {
        for (o, z) in [f.0, f.1].map(|f| f.unwrap_or((Vec3::ZERO, Vec3::ZERO))) {
            assert!(near(o, Vec3::new(100.0, 50.0, 2.0)) && near(z, Vec3::Z), "step {i}: {f:?}");
        }
    }
}

// ---- Two arms jointed together, the arm part edited: every joint type keeps its origins
// together, for each way the joint dialog picks an origin, inside one design and through a link.

/// The arm: a plate `L` × 20 × `T` (100 × 20 × 5 to start) along +X from the origin, with a Ø 6
/// hole through it at x = `HX` (10) and another 10 mm in from its far end. Made at the active
/// component.
pub(crate) fn arm_part(s: &mut Session) {
    for (name, e) in [("T", "5 mm"), ("L", "100 mm"), ("HX", "10 mm")] {
        run(s, "parameters.add", json!({"name": name, "expression": e}));
    }
    run(s, "solid.box", json!({"length": "L", "width": 20, "height": "T", "corner": [0, -10, 0], "body_name": "Arm"}));
    run(s, "solid.cylinder", json!({"diameter": 6, "height": 100, "base": ["HX", 0, -50], "operation": "cut"}));
    run(s, "solid.cylinder", json!({"diameter": 6, "height": 100, "base": ["L - 10 mm", 0, -50], "operation": "cut"}));
}

/// The arm's edits, (parameter, value), applied in turn: thicker (A's top face moves along the
/// joint's axis, as does B's bottom hole rim relative to A), A's hole moves sideways, and the arm
/// grows so its far end (B's hole, edge and corner) moves sideways.
pub(crate) const ARM_EDITS: [(&str, f64); 4] = [("T", 12.0), ("HX", 15.0), ("L", 140.0), ("T", 3.0)];

/// The joint types tried (each between its own pair of arms) and as-built.
pub(crate) const ARM_KINDS: [&str; 9] = ["rigid", "revolute", "slider", "cylindrical", "pin_slot", "planar", "ball", "point_on_plane", "as_built"];

/// How the joint dialog picks an origin on arm A's top (a) and arm B's bottom (b), as world
/// params, for the arms at A (0, y, 0) and B (0, y + 60, 0) in the arm's starting size.
pub(crate) fn arm_origins(form: &str, y: f64) -> (serde_json::Value, serde_json::Value) {
    let b = y + 60.0;
    match form {
        // A hole's rim (a round edge): its centre and the way out of the material.
        "rim" => (json!({"circle": [10, y, 5], "z": [0, 0, 1]}), json!({"circle": [90, b, 0], "z": [0, 0, -1]})),
        // A hole's wall.
        "wall" => (json!({"circle": [13, y, 4]}), json!({"circle": [93, b, 1]})),
        // The plate's top and bottom faces.
        "face" => (json!({"face": [50, y + 5.0, 5]}), json!({"face": [50, b + 5.0, 0]})),
        // A straight edge: its middle, z along it.
        "edge" => (json!({"edge": [50, y + 10.0, 5], "z": [1, 0, 0]}), json!({"edge": [50, b + 10.0, 0], "z": [1, 0, 0]})),
        // A corner (a vertex), z along the part's +Z.
        _ => (
            json!({"edge": [100, y + 10.0, 5], "corner": [100, y + 10.0, 5], "z": [0, 0, 1]}),
            json!({"edge": [100, b + 10.0, 0], "corner": [100, b + 10.0, 0], "z": [0, 0, 1]}),
        ),
    }
}

pub(crate) const ARM_FORMS: [&str; 5] = ["rim", "wall", "face", "edge", "corner"];

/// One joint of each of [`ARM_KINDS`] between arm pairs `pairs` (A, B occurrences), its origins
/// picked as `form` picks them.
pub(crate) fn joint_arms(s: &mut Session, form: &str, pairs: &[(u64, u64)]) {
    for (k, (kind, (a, b))) in ARM_KINDS.iter().zip(pairs).enumerate() {
        let (mut pa, mut pb) = arm_origins(form, 120.0 * k as f64);
        pa["occurrence"] = json!(a);
        pb["occurrence"] = json!(b);
        let name = format!("{form}-{kind}");
        let r = if *kind == "as_built" {
            run(s, "joint.as_built", json!({"type": "rigid", "name": name, "a": a, "b": b, "at": pa}))
        } else {
            run(s, "joint.create", json!({"type": kind, "name": name, "a": pa, "b": pb}))
        };
        assert_eq!(r["errors"], json!([]), "{name}: {r}");
    }
}

/// The arm's size as edited so far: T, HX, L (mm).
pub(crate) fn arm_size(edits: &[(&str, f64)]) -> (f64, f64, f64) {
    let at = |n: &str, d: f64| edits.iter().rev().find(|(m, _)| *m == n).map_or(d, |(_, v)| *v);
    (at("T", 5.0), at("HX", 10.0), at("L", 100.0))
}

/// Where `form` picked A's and B's origins, on the arm as it is now (T, HX, L): component
/// coordinates of the geometry itself (the hole's rim, the plate's top face's centre, …).
fn arm_spots(form: &str, (t, hx, l): (f64, f64, f64)) -> (Vec3, Vec3) {
    match form {
        "rim" | "wall" => (Vec3::new(hx, 0.0, t), Vec3::new(l - 10.0, 0.0, 0.0)),
        "face" => (Vec3::new(l / 2.0, 0.0, t), Vec3::new(l / 2.0, 0.0, 0.0)),
        "edge" => (Vec3::new(l / 2.0, 10.0, t), Vec3::new(l / 2.0, 10.0, 0.0)),
        _ => (Vec3::new(l, 10.0, t), Vec3::new(l, 10.0, 0.0)),
    }
}

/// Each joint between `pairs` checked against the arm's geometry as it is now (`size`): A's
/// origin is on the rim, face, edge or corner it was picked on; B's is on its own and sits on
/// A's, z against z (point-on-plane: on A's plane; as-built: B keeps its place from A's
/// origin). The joints that aren't, with why.
pub(crate) fn arms_apart(s: &mut Session, edit: &str, form: &str, pairs: &[(u64, u64)], size: (f64, f64, f64)) -> Vec<String> {
    let list = run(s, "joint.list", json!({}));
    let mut out: Vec<String> = list["errors"].as_array().into_iter().flatten().map(|e| format!("{edit}: {} {}", e["name"], e["why"])).collect();
    let v = |f: &serde_json::Value, k: &str| {
        Vec3::new(f[k][0].as_f64().unwrap_or(f64::NAN), f[k][1].as_f64().unwrap_or(f64::NAN), f[k][2].as_f64().unwrap_or(f64::NAN))
    };
    let world = |o: u64, p: Vec3| solidcraft_doc::apply_point(&solidcraft_doc::joints::occurrence_world(&s.doc, o), p);
    let (sa, sb) = arm_spots(form, size);
    let (sa0, _) = arm_spots(form, arm_size(&[]));
    for (kind, (a, b)) in ARM_KINDS.iter().zip(pairs) {
        let name = format!("{form}-{kind}");
        let Some(j) = list["joints"].as_array().into_iter().flatten().find(|j| j["name"] == name.as_str()) else {
            out.push(format!("{edit}: {name} is gone"));
            continue;
        };
        let (oa, ob, za, zb) = (v(&j["frame_a"], "origin"), v(&j["frame_b"], "origin"), v(&j["frame_a"], "z"), v(&j["frame_b"], "z"));
        let (at_a, at_b) = (world(*a, sa), world(*b, sb));
        let mut why = Vec::new();
        if oa.dist(at_a) > 1e-6 {
            why.push(format!("A's origin at {oa:?}, its geometry at {at_a:?}"));
        }
        match *kind {
            "as_built" => {
                // B was made 60 mm along Y from A, the origin wherever it was on A.
                let want = world(*a, Vec3::new(0.0, 60.0, 0.0) + sa - sa0);
                if world(*b, Vec3::ZERO).dist(want) > 1e-6 || oa.dist(ob) > 1e-6 {
                    why.push(format!("B at {:?}, not {want:?}", world(*b, Vec3::ZERO)));
                }
            }
            "point_on_plane" => {
                if (at_b - at_a).dot(za).abs() > 1e-6 {
                    why.push(format!("B's spot {at_b:?} is off A's plane at {at_a:?} z {za:?}"));
                }
            }
            _ => {
                if ob.dist(at_b) > 1e-6 || at_a.dist(at_b) > 1e-6 || za.dot(zb) > -1.0 + 1e-9 {
                    why.push(format!("B's origin at {ob:?} z {zb:?}, its geometry at {at_b:?}, A's at {at_a:?} z {za:?}"));
                }
            }
        }
        if !why.is_empty() {
            out.push(format!("{edit}: {name}: {}", why.join("; ")));
        }
    }
    out
}

/// The A, B arm occurrence pairs (one per joint type), A grounded, from the arm's first
/// occurrence `first` (at the origin).
pub(crate) fn arm_pairs(s: &mut Session, first: u64) -> Vec<(u64, u64)> {
    run(s, "occurrence.ground", json!({"occurrence": first, "grounded": true}));
    (0..ARM_KINDS.len())
        .map(|k| {
            let y = 120.0 * k as f64;
            let a = if k == 0 {
                first
            } else {
                run(s, "occurrence.copy", json!({"occurrence": first, "translate": [0, y, 0]}))["occurrence"].as_u64().unwrap_or(0)
            };
            run(s, "occurrence.ground", json!({"occurrence": a, "grounded": true}));
            let b = run(s, "occurrence.copy", json!({"occurrence": first, "translate": [0, y + 60.0, 0]}))["occurrence"].as_u64().unwrap_or(0);
            (a, b)
        })
        .collect()
}

/// Two arms jointed top face to bottom face, the arm's thickness, holes and length changed in the
/// design: every joint re-solves onto the moved faces, edges and corners (the joint dialog kept
/// a round edge, a straight one and a corner as points where they were picked, so a thicker
/// arm overlapped the one jointed to it).
#[test]
fn jointed_arms_stay_together_as_the_arm_changes() {
    let mut apart = Vec::new();
    for form in ARM_FORMS {
        let mut s = Session::default();
        run(&mut s, "component.create", json!({"name": "Arm"}));
        arm_part(&mut s);
        run(&mut s, "component.activate", json!({"component": "root"}));
        let first = occ(&s, "Arm");
        let pairs = arm_pairs(&mut s, first);
        joint_arms(&mut s, form, &pairs);
        apart.extend(arms_apart(&mut s, "as made", form, &pairs, arm_size(&[])));
        for (i, (name, value)) in ARM_EDITS.into_iter().enumerate() {
            run(&mut s, "parameters.change", json!({"name": name, "expression": format!("{value} mm")}));
            apart.extend(arms_apart(&mut s, &format!("{name} = {value}"), form, &pairs, arm_size(&ARM_EDITS[..=i])));
        }
    }
    assert!(apart.is_empty(), "{} joints apart:\n{}", apart.len(), apart.join("\n"));
}
