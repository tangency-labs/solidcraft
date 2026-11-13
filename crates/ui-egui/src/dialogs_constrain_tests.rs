use serde_json::json;
use solidcore_record::Vec3;
use solidcraft_engine::Session;

use super::*;
use crate::Services;
use crate::dialogs::{Kind, apply_commands, preview_commands};

const COLORS: crate::preview::Colors = crate::preview::Colors {
    body: [1, 1, 1, 255],
    added: [2, 2, 2, 255],
    new_body: [2, 2, 2, 255],
    removed: [3, 3, 3, 100],
    edge: [0, 0, 0, 255],
    ghost: true,
    tint_new: true,
    cut_tint: true,
};

/// A grounded plate (100 x 100 x 10, top at z 10) and a 20 mm block above it at (30, 40, 50).
fn plate_and_block() -> SolveApp {
    let mut s = Session::default();
    let mut run = |id: &str, p: Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"));
    for (name, size, corner) in [("Plate", [100, 100, 10], [0, 0, 0]), ("Block", [20, 20, 20], [30, 40, 50])] {
        run("component.activate", json!({"component": "root"}));
        run("component.create", json!({ "name": name }));
        run("solid.box", json!({"length": size[0], "width": size[1], "height": size[2], "corner": corner}));
    }
    run("component.activate", json!({"component": "root"}));
    run("occurrence.ground", json!({"occurrence": "Plate:1", "grounded": true}));
    SolveApp::new(s, Services::default())
}

/// A pick of the face a point of the exact geometry lies on (the nearest face of any body).
fn face(app: &SolveApp, p: Vec3) -> Sel {
    let st = app.session.world_state().unwrap();
    let mut best: Option<(f64, Sel)> = None;
    for b in &st.bodies {
        let m = b.mesh();
        for (t, f) in m.triangles.iter().zip(&m.tri_face) {
            let Some([a, bb, c]) = m.tri(t) else { continue };
            let d = crate::preview::point_triangle_dist(p, a, bb, c);
            if best.as_ref().is_none_or(|(bd, _)| d < *bd) {
                best = Some((d, Sel::Face { body: b.name.clone(), index: *f as usize, point: p }));
            }
        }
    }
    best.map(|x| x.1).unwrap()
}

/// The Constrain dialog with the plate's top and the block's bottom picked.
fn picked(app: &mut SolveApp) {
    app.start("assembly.constraint.create");
    let mut d = app.dialog.take().unwrap();
    for (i, p) in [Vec3::new(5.0, 5.0, 10.0), Vec3::new(31.0, 41.0, 50.0)].into_iter().enumerate() {
        d.active = i;
        let pick = face(app, p);
        d.pick(&app.session, pick);
    }
    app.dialog = Some(d);
}

fn form(app: &mut SolveApp) -> &mut ConstrainForm {
    match app.dialog.as_mut().map(|d| &mut d.kind) {
        Some(Kind::Assembly(Asm::Constrain(f))) => f,
        _ => panic!("the Constrain dialog is not open"),
    }
}

fn shown(app: &mut SolveApp) -> (Vec<String>, egui::FullOutput) {
    let ctx = crate::fonts::test_context();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0));
    let frame = |app: &mut SolveApp| {
        let input = egui::RawInput { screen_rect: Some(screen), ..Default::default() };
        crate::test_support::frame(&ctx, input, |ui| crate::dialogs::show(app, ui.ctx()))
    };
    frame(app);
    let out = frame(app);
    (crate::test_support::text_shapes(&out).map(|t| t.galley.text().to_string()).collect(), out)
}

fn occ(app: &SolveApp, name: &str) -> u64 {
    app.session.doc.occurrences.iter().find(|o| o.name == name).map(|o| o.id).unwrap_or(0)
}

/// The rules offered are those the commands accept (the engine's one table), the likeliest first:
/// every rule offered for a pair of shapes is one `fit` keeps, and every other one it refuses.
#[test]
fn the_type_row_offers_the_rules_the_references_fit() {
    use Constraint::*;
    use RefShape::*;
    assert_eq!(Constraint::fitting(Plane, Plane), [Mate, Flush, Distance, Parallel, Perpendicular, Angle]);
    assert_eq!(Constraint::fitting(Round, Round).first(), Some(&Concentric));
    assert_eq!(Constraint::fitting(Plane, Round).first(), Some(&Tangent));
    assert_eq!(Constraint::fitting(Point, Plane).first(), Some(&Distance));
    assert_eq!(Constraint::fitting(Round, Point).first(), Some(&PointOnAxis));
    // An orientation rule needs a direction on both sides: an edge has its face's, a point none.
    for c in [Parallel, Perpendicular, Angle] {
        assert!(Constraint::fitting(Edge, Edge).contains(&c) && Constraint::fitting(Edge, Plane).contains(&c), "{c:?}");
        assert!(!Constraint::fitting(Point, Point).contains(&c) && !Constraint::fitting(Point, Plane).contains(&c), "{c:?}");
    }
    // An edge still gives a position, as a point does.
    assert_eq!(Constraint::fitting(Plane, Edge).first(), Some(&Distance));
    assert_eq!(Constraint::fitting(Axis, Edge).first(), Some(&PointOnAxis));
    for a in [Plane, Round, Point, Axis, Edge] {
        for b in [Plane, Round, Point, Axis, Edge] {
            let offered = Constraint::fitting(a, b);
            for c in Constraint::ALL {
                assert_eq!(offered.contains(&c), c.fit(a, b).is_some(), "{c:?} for {a:?}, {b:?}");
            }
        }
    }
    // The dialog's picks map to the shapes the engine gives their origins: a datum plane is a
    // face centre with a `plane`, a datum axis an edge's middle with an `axis`.
    let snap = |kind, param| SnapPt { at: Vec3::ZERO, z: Vec3::Z, kind, param };
    assert_eq!(shape(&snap(SnapKind::FaceCenter, json!({"face": [0, 0, 0]}))), Plane);
    assert_eq!(shape(&snap(SnapKind::FaceCenter, json!({"occurrence": 0, "plane": "XY"}))), Plane);
    assert_eq!(shape(&snap(SnapKind::CircleCenter, json!({"circle": [0, 0, 0]}))), Round);
    assert_eq!(shape(&snap(SnapKind::EdgeMid, json!({"occurrence": 0, "axis": "Z"}))), Axis);
    assert_eq!(shape(&snap(SnapKind::EdgeMid, json!({"edge": [0, 0, 0]}))), Edge);
    assert_eq!(shape(&snap(SnapKind::Vertex, json!({"edge": [0, 0, 0], "corner": "e2"}))), Edge);
    assert_eq!(shape(&snap(SnapKind::Vertex, json!({"point": [0, 0, 0], "z": [0, 0, 1]}))), Point);
    // A datum axis and a round face: concentric first, as a shaft on an axis.
    assert_eq!(Constraint::fitting(Axis, Round).first(), Some(&Concentric));
}

/// Two faces picked: Mate is offered first, OK makes it with both references (and their
/// placements), and the block lands on the plate.
#[test]
fn two_faces_picked_make_a_mate() {
    let mut app = plate_and_block();
    picked(&mut app);
    let (text, _) = shown(&mut app);
    assert!(text.iter().any(|t| t == label(Constraint::Mate)), "{text:?}");
    let d = app.dialog.as_ref().unwrap();
    let c = apply_commands(&app, d).unwrap();
    let (id, p) = &c[0];
    assert_eq!((id.as_str(), p["type"].as_str()), ("assembly.constraint.create", Some("mate")), "{p}");
    assert!(p["a"]["face"].is_array() && p["b"]["occurrence"].is_u64(), "{p}");
    for (id, p) in c {
        app.run(&id, p).unwrap();
    }
    let j = app.session.doc.assembly.joints.first().unwrap();
    assert_eq!(j.constraint, Some(Constraint::Mate));
    // The preview of the same command moves the block (it is in the placing commands).
    let mut app = plate_and_block();
    picked(&mut app);
    let d = app.dialog.as_ref().unwrap();
    let cmds = preview_commands(&app, d).unwrap();
    assert!(crate::dialogs_assembly::world_preview(&app.session, &cmds, COLORS).is_some(), "the constraint is previewed on the placed model");
}

/// The status line reads the preview's reply while a rule is previewed, and the saved design
/// otherwise; it is drawn inside the dialog.
#[test]
fn the_status_line_reads_the_preview() {
    let mut app = plate_and_block();
    picked(&mut app);
    let block = occ(&app, "Block:1");
    app.preview.placed = Some(json!({"dof": [{"occurrence": block, "dof": 3}], "conflicts": []}));
    let (text, out) = shown(&mut app);
    let want = solidcraft_ui_text::format("constrain.dof", &[("part", "Block:1"), ("count", "3")]);
    assert!(text.contains(&want), "{text:?}");
    let line = crate::test_support::text_shapes(&out).find(|t| t.galley.text() == want).unwrap();
    let at = line.galley.rect.translate(line.pos.to_vec2());
    let dialog = app.viewport.dialog_rect.expect("dialog bounds");
    assert!(dialog.contains_rect(at), "the status line at {at:?} is outside the dialog {dialog:?}");
    // Nothing previewed: the saved design, where the block is free.
    app.preview.placed = None;
    let (text, _) = shown(&mut app);
    let free = solidcraft_ui_text::format("constrain.dof", &[("part", "Block:1"), ("count", "6")]);
    assert!(text.contains(&free), "{text:?}");
    // A conflict names the other constraint.
    app.preview.placed = Some(json!({"dof": [], "conflicts": [{"joint": 999, "name": "Other", "why": "off"}]}));
    let (text, _) = shown(&mut app);
    assert!(!text.iter().any(|t| t.contains("Other")), "a conflict of another part's joint is not this part's: {text:?}");
}

/// The constraints between the two parts are listed (a long name keeps its buttons in the
/// panel), and Edit loads one: OK then changes it through assembly.constraint.edit.
#[test]
fn a_listed_constraint_loads_for_editing() {
    let mut app = plate_and_block();
    let p = json!({"type": "mate", "a": {"occurrence": "Plate:1", "face": [5, 5, 10]}, "b": {"occurrence": "Block:1", "face": [31, 41, 50]},
        "name": "A constraint with a long name between the plate and the block"});
    app.run("assembly.constraint.create", p).unwrap();
    let id = app.session.doc.assembly.joints.first().unwrap().id;
    app.start("assembly.constraint.create");
    let (plate, block) = (occ(&app, "Plate:1"), occ(&app, "Block:1"));
    form(&mut app).pair = Some((plate, block));
    let (text, out) = shown(&mut app);
    assert!(text.iter().any(|t| t.starts_with("A constraint with a long name")), "{text:?}");
    let dialog = app.viewport.dialog_rect.expect("dialog bounds");
    let delete = crate::test_support::text_shapes(&out).find(|t| t.galley.text() == solidcraft_ui_text::tr_label("Delete")).unwrap();
    assert!(!delete.galley.elided, "Delete is cut off");
    assert!(dialog.contains_rect(delete.galley.rect.translate(delete.pos.to_vec2())), "Delete is outside the dialog");
    // Edit loads it: its references are the picks, and OK changes it.
    let mut d = app.dialog.take().unwrap();
    let mut active = d.active;
    if let Kind::Assembly(Asm::Constrain(f)) = &mut d.kind {
        load(&app, f, &mut d.inputs, &mut active, id);
        f.rule = Some(Constraint::Distance);
        f.offset = "15 mm".into();
    }
    app.dialog = Some(d);
    let d = app.dialog.as_ref().unwrap();
    let c = apply_commands(&app, d).unwrap();
    assert_eq!(c[0].0, "assembly.constraint.edit", "{c:?}");
    assert_eq!((c[0].1["constraint"].clone(), c[0].1["type"].clone(), c[0].1["offset"].clone()), (json!(id), json!("distance"), json!("15 mm")));
    for (cmd, p) in c {
        app.run(&cmd, p).unwrap();
    }
    assert_eq!(app.session.doc.assembly.joints.first().and_then(|j| j.constraint), Some(Constraint::Distance));
}

/// Picking something else after loading a constraint makes a new rule, not a change.
#[test]
fn a_new_pick_after_loading_makes_a_new_rule() {
    let mut app = plate_and_block();
    let p = json!({"type": "mate", "a": {"occurrence": "Plate:1", "face": [5, 5, 10]}, "b": {"occurrence": "Block:1", "face": [31, 41, 50]}});
    app.run("assembly.constraint.create", p).unwrap();
    let id = app.session.doc.assembly.joints.first().unwrap().id;
    app.start("assembly.constraint.create");
    let mut d = app.dialog.take().unwrap();
    let mut active = d.active;
    if let Kind::Assembly(Asm::Constrain(f)) = &mut d.kind {
        load(&app, f, &mut d.inputs, &mut active, id);
    }
    // The block's side face instead of its bottom (the block now sits on the plate).
    d.active = 1;
    let pick = face(&app, Vec3::new(50.0, 50.0, 20.0));
    d.pick(&app.session, pick);
    app.dialog = Some(d);
    let d = app.dialog.as_ref().unwrap();
    let c = apply_commands(&app, d).unwrap();
    assert_eq!(c[0].0, "assembly.constraint.create", "{c:?}");
}

/// Apply runs the rule and keeps the two parts: the picks clear, the first reference is active,
/// the defaults come back (the gap typed for one rule is not the next one's).
#[test]
fn apply_keeps_the_pair_and_starts_the_next_rule_fresh() {
    let mut h = crate::scenario::Harness::new();
    h.app = plate_and_block();
    picked(&mut h.app);
    {
        let f = form(&mut h.app);
        f.rule = Some(Constraint::Distance);
        f.offset = "-20 mm".into();
    }
    h.frames(3);
    h.step(&json!({"click": {"handle": "dialog:Apply"}})).unwrap();
    h.frames(2);
    assert_eq!(h.app.session.doc.assembly.joints.first().and_then(|j| j.constraint), Some(Constraint::Distance));
    let d = h.app.dialog.as_ref().expect("the dialog stays open");
    assert!(d.inputs.iter().all(|i| i.items.is_empty()) && d.active == 0, "{:?}", d.inputs);
    let f = form(&mut h.app);
    assert_eq!((f.offset.as_str(), f.rule, f.pair.is_some()), ("0 mm", None, true));
}

/// A datum plane is a reference of its part: a plate's XY plane and the block's bottom name the
/// plate and the block, so the rule made between them is listed and the block's freedom is shown.
#[test]
fn a_datum_reference_names_its_part() {
    let mut app = plate_and_block();
    let p = json!({"type": "mate", "a": {"occurrence": "Plate:1", "plane": "XY"}, "b": {"occurrence": "Block:1", "face": [31, 41, 50]}});
    app.run("assembly.constraint.create", p).unwrap();
    app.start("assembly.constraint.create");
    let mut d = app.dialog.take().unwrap();
    d.active = 0;
    d.pick(&app.session, Sel::Plane { name: "Plate:1/XY".into() });
    d.active = 1;
    // The block now stands on the plate's XY plane (z 0 to 20, through the plate): its top, at z 20,
    // is its own face (its bottom lies on the plate's).
    let pick = face(&app, Vec3::new(40.0, 50.0, 20.0));
    d.pick(&app.session, pick);
    app.dialog = Some(d);
    let f = match app.dialog.as_ref().map(|d| &d.kind) {
        Some(Kind::Assembly(Asm::Constrain(f))) => f.clone(),
        _ => panic!("the Constrain dialog is not open"),
    };
    let r = refs(&app, &f, &app.dialog.as_ref().unwrap().inputs).expect("both references resolve");
    assert_eq!(r.parts, [occ(&app, "Plate:1"), occ(&app, "Block:1")]);
    let (text, _) = shown(&mut app);
    assert!(text.iter().any(|t| t.starts_with("Mate1")), "the rule between them is listed: {text:?}");
    let block = solidcraft_ui_text::format("constrain.dof", &[("part", "Block:1"), ("count", "3")]);
    assert!(text.contains(&block), "{text:?}");
}

/// A constraint with a point side loads for editing: the point keeps its saved origin (no snap is
/// read back from it), the Type row stays, and Change edits it.
#[test]
fn a_point_side_loads_for_editing() {
    let mut app = plate_and_block();
    let p = json!({"type": "distance", "a": {"occurrence": "Plate:1", "face": [5, 5, 10]},
        "b": {"occurrence": "Block:1", "point": [30, 40, 50]}, "offset": 40});
    app.run("assembly.constraint.create", p).unwrap();
    let id = app.session.doc.assembly.joints.first().unwrap().id;
    app.start("assembly.constraint.create");
    let mut d = app.dialog.take().unwrap();
    let mut active = d.active;
    if let Kind::Assembly(Asm::Constrain(f)) = &mut d.kind {
        load(&app, f, &mut d.inputs, &mut active, id);
        f.offset = "25 mm".into();
    }
    app.dialog = Some(d);
    let (text, _) = shown(&mut app);
    assert!(text.iter().any(|t| t == label(Constraint::Distance)), "the Type row is shown: {text:?}");
    let c = apply_commands(&app, app.dialog.as_ref().unwrap()).unwrap();
    assert_eq!((c[0].0.as_str(), c[0].1["constraint"].clone(), c[0].1["offset"].clone()), ("assembly.constraint.edit", json!(id), json!("25 mm")));
    for (cmd, p) in c {
        app.run(&cmd, p).unwrap();
    }
    let j = app.session.doc.assembly.joints.first().unwrap();
    assert_eq!((j.constraint, j.offset), (Some(Constraint::Distance), 25.0));
}

/// Editing only a flipped mate's gap leaves the rule's type out, so the part stays turned over;
/// changing the rule names it.
#[test]
fn a_gap_edit_keeps_a_flipped_mate_flipped() {
    let mut app = plate_and_block();
    let p = json!({"type": "mate", "a": {"occurrence": "Plate:1", "face": [5, 5, 10]}, "b": {"occurrence": "Block:1", "face": [31, 41, 50]}, "flip": true});
    app.run("assembly.constraint.create", p).unwrap();
    let j = app.session.doc.assembly.joints.first().unwrap();
    let (id, flipped) = (j.id, j.flip);
    app.start("assembly.constraint.create");
    let mut d = app.dialog.take().unwrap();
    let mut active = d.active;
    if let Kind::Assembly(Asm::Constrain(f)) = &mut d.kind {
        load(&app, f, &mut d.inputs, &mut active, id);
        f.offset = "5 mm".into();
    }
    app.dialog = Some(d);
    let c = apply_commands(&app, app.dialog.as_ref().unwrap()).unwrap();
    assert!(c[0].1.get("type").is_none(), "an unchanged rule is not sent again: {}", c[0].1);
    for (cmd, p) in c {
        app.run(&cmd, p).unwrap();
    }
    let j = app.session.doc.assembly.joints.first().unwrap();
    assert_eq!((j.flip, j.offset), (flipped, 5.0), "still turned over, 5 mm off");
    // Another rule is named.
    form(&mut app).rule = Some(Constraint::Flush);
    let c = apply_commands(&app, app.dialog.as_ref().unwrap()).unwrap();
    assert_eq!(c[0].1["type"], json!("flush"), "{}", c[0].1);
}

/// A refused Apply keeps both picks and the values typed, with the reason, so one wrong value is
/// corrected without picking again.
#[test]
fn a_refused_apply_keeps_the_picks() {
    let mut h = crate::scenario::Harness::new();
    h.app = plate_and_block();
    picked(&mut h.app);
    form(&mut h.app).offset = "no such length".into();
    h.frames(3);
    h.step(&json!({"click": {"handle": "dialog:Apply"}})).unwrap();
    h.frames(2);
    assert!(h.app.session.doc.assembly.joints.is_empty());
    let d = h.app.dialog.as_ref().expect("the dialog stays open");
    assert!(d.inputs.iter().all(|i| i.items.len() == 1), "{:?}", d.inputs);
    let f = form(&mut h.app);
    assert_eq!(f.offset, "no such length");
    assert!(f.error.is_some());
}

/// The moving part is the one the engine moves: the second reference, or the first when the rule
/// wants the plane first (a point picked before the plate's face for a distance).
#[test]
fn the_freedom_line_follows_the_moving_part() {
    let r = Refs { shapes: [RefShape::Point, RefShape::Plane], parts: [7, 3], params: None };
    assert_eq!(held_and_moving(&r, Some(Constraint::Distance)), (3, 7), "swapped: the point's part moves");
    let r = Refs { shapes: [RefShape::Plane, RefShape::Plane], parts: [3, 7], params: None };
    assert_eq!(held_and_moving(&r, Some(Constraint::Mate)), (3, 7));
    assert_eq!(held_and_moving(&r, None), (3, 7));
}
