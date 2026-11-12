use serde_json::{Value, json};

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn refused(s: &mut Session, id: &str, p: Value) -> String {
    match s.execute(id, &p) {
        Ok(v) => panic!("{id} {p} was not refused: {v}"),
        Err(e) => e.to_string(),
    }
}

/// A block 100 x 60 x 30 from the origin, started unfolding on its bottom face: there it lies
/// with its centre over the origin, so its bottom edges' middles are (0, ∓30, 0) and (±50, 0, 0).
fn block() -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 100, "width": 60, "height": 30, "corner": [0, 0, 0], "body_name": "Block"}));
    run(&mut s, "print.unfold.start", json!({"body": "Block", "face": [50, 30, 0]}));
    s
}

fn counts(v: &Value) -> (u64, usize, usize, usize) {
    let n = |k: &str| v[k].as_array().map_or(0, Vec::len);
    (v["attached"].as_u64().unwrap_or(0), n("bends"), n("cuts"), n("open"))
}

fn bbox(s: &mut Session, body: &str) -> ([f64; 3], [f64; 3]) {
    let m = run(s, "inspect.measure", json!({"bodies": [body]}));
    let read = |k: &str| -> [f64; 3] {
        let a = &m["bodies"][0]["bbox"][k];
        [a[0].as_f64().unwrap_or(f64::NAN), a[1].as_f64().unwrap_or(f64::NAN), a[2].as_f64().unwrap_or(f64::NAN)]
    };
    (read("min"), read("max"))
}

#[test]
fn start_copies_the_body_into_a_design_of_its_own_lying_on_its_base_face() {
    let mut s = block();
    // The new design holds the body alone, on Z = 0, its centre of mass over the origin.
    let (lo, hi) = bbox(&mut s, "Block");
    let want = ([-50.0, -30.0, 0.0], [50.0, 30.0, 30.0]);
    assert!((0..3).all(|i| (lo[i] - want.0[i]).abs() < 1e-6 && (hi[i] - want.1[i]).abs() < 1e-6), "{lo:?} {hi:?}");
    assert_eq!(s.doc.features.len(), 1);
    // It remembers where the body came from, and what it measured there: 100 x 60 x 30.
    let unfold = s.doc.print_unfold.clone().unwrap_or_else(|| panic!("no unfolded print"));
    let source = unfold.source.unwrap_or_else(|| panic!("no source"));
    assert_eq!((source.body.as_str(), source.stamp.faces), ("Block", 6));
    assert!((source.stamp.volume - 180_000.0).abs() < 1e-3 && (source.stamp.area - 21_600.0).abs() < 1e-3, "{:?}", source.stamp);
    // Only the base face is on the pattern: its four edges are cuts, the other eight open.
    let v = run(&mut s, "print.unfold.state", json!({}));
    assert_eq!((v["step"].as_str(), v["faces"].as_u64(), v["complete"].as_bool()), (Some("edges"), Some(6), Some(false)), "{v}");
    assert_eq!(counts(&v), (1, 0, 4, 8), "{v}");
    // The five faces not on it are listed by their outlines, four corners each.
    let missing = v["missing"].as_array().cloned().unwrap_or_default();
    assert!(missing.len() == 5 && missing.iter().all(|lp| lp.as_array().map(Vec::len) == Some(4)), "{v}");
    // Every edge of a block folds by a quarter turn.
    assert!(
        v["cuts"].as_array().into_iter().flatten().all(|e| e["foldable"] == true && (e["angle"].as_f64().unwrap_or(0.0) - 90.0).abs() < 1e-9),
        "{v}"
    );

    // On a side face (100 long, at x = 100) it stands 100 high on a 60 x 30 footprint.
    let mut side = Session::default();
    run(&mut side, "solid.box", json!({"length": 100, "width": 60, "height": 30, "corner": [0, 0, 0], "body_name": "Block"}));
    run(&mut side, "print.unfold.start", json!({"body": "Block", "face": [100, 30, 15]}));
    let (lo, hi) = bbox(&mut side, "Block");
    let mut foot = [hi[0] - lo[0], hi[1] - lo[1]];
    foot.sort_by(f64::total_cmp);
    assert!(lo[2].abs() < 1e-6 && (hi[2] - 100.0).abs() < 1e-6 && (foot[0] - 30.0).abs() < 1e-6 && (foot[1] - 60.0).abs() < 1e-6, "{lo:?} {hi:?}");
}

#[test]
fn a_bend_hangs_the_face_across_it_on_the_pattern_and_cuts_that_faces_other_edges() {
    let mut s = block();
    // The front wall hangs on the base face: its top and its two upright edges are cuts now,
    // next to the base face's other three. The five edges of the faces not reached stay open.
    let v = run(&mut s, "print.unfold.bend", json!({"edge": [0, -30, 0]}));
    assert_eq!((v["changed"].as_str(), counts(&v)), (Some("added"), (2, 1, 6, 5)), "{v}");
    // An edge between two faces not on the pattern can't be a bend yet.
    let e = refused(&mut s, "print.unfold.bend", json!({"edge": [0, 30, 30]}));
    assert!(e.contains("neither face"), "{e}");
    // The other three walls: every edge is a bend or a cut, but the top face hangs nowhere.
    for edge in [[50, 0, 0], [0, 30, 0], [-50, 0, 0]] {
        run(&mut s, "print.unfold.bend", json!({"edge": edge}));
    }
    let v = run(&mut s, "print.unfold.state", json!({}));
    assert_eq!((counts(&v), v["complete"].as_bool()), ((5, 4, 8, 0), Some(false)), "{v}");
    let e = refused(&mut s, "print.unfold.surface", json!({}));
    assert!(e.contains("1 of the body's 6 faces"), "{e}");
    // The top face on the front wall: the pattern is complete, five bends and seven cuts.
    let v = run(&mut s, "print.unfold.bend", json!({"edge": [0, -30, 30]}));
    assert_eq!((counts(&v), v["complete"].as_bool()), ((6, 5, 7, 0), Some(true)), "{v}");
    // A cut between two faces of the pattern moves the face farther from the base face: the
    // top face hangs on the back wall now, and its edge to the front wall is a cut again.
    let v = run(&mut s, "print.unfold.bend", json!({"edge": [0, 30, 30]}));
    assert_eq!((v["changed"].as_str(), counts(&v)), (Some("moved"), (6, 5, 7, 0)), "{v}");
    let is_bend = |v: &Value, y: f64| {
        v["bends"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|e| (e["start"][1].as_f64().unwrap_or(0.0) - y).abs() < 1e-6 && e["start"][2] == 30.0 && e["end"][2] == 30.0)
    };
    assert!(is_bend(&v, 30.0) && !is_bend(&v, -30.0), "{v}");
    // A bend clicked again is a cut again, and what hung by it is off the pattern: the back
    // wall and the top face on it. The edge between those two is open again.
    let v = run(&mut s, "print.unfold.bend", json!({"edge": [0, 30, 0]}));
    assert_eq!((v["changed"].as_str(), counts(&v)), (Some("removed"), (4, 3, 8, 1)), "{v}");
    // Each click is one step to undo.
    run(&mut s, "edit.undo", json!({}));
    let v = run(&mut s, "print.unfold.state", json!({}));
    assert_eq!(counts(&v), (6, 5, 7, 0), "{v}");
}

#[test]
fn the_surface_model_is_every_face_as_a_sheet_of_its_own_with_the_solid_hidden() {
    let mut s = block();
    for edge in [[0, -30, 0], [50, 0, 0], [0, 30, 0], [-50, 0, 0], [0, -30, 30]] {
        run(&mut s, "print.unfold.bend", json!({"edge": edge}));
    }
    let v = run(&mut s, "print.unfold.surface", json!({}));
    assert_eq!(v["step"], "surface", "{v}");
    // Six sheets next to the solid, together the block's skin: 2·(100·60 + 100·30 + 60·30).
    let names: Vec<String> = s.model.state().bodies.iter().map(|b| b.name.clone()).filter(|n| n != "Block").collect();
    assert_eq!(names.len(), 6, "{names:?}");
    let m = run(&mut s, "inspect.measure", json!({"bodies": names}));
    let area: f64 = m["bodies"].as_array().into_iter().flatten().map(|b| b["area_mm2"].as_f64().unwrap_or(f64::NAN)).sum();
    assert!((area - 21_600.0).abs() < 1e-3, "{area}");
    assert!(m["bodies"].as_array().into_iter().flatten().all(|b| b["closed"] == false), "{m}");
    assert!(s.visibility.hidden_bodies.contains(&"Block".to_string()));
    // The bends can't change under it.
    let e = refused(&mut s, "print.unfold.bend", json!({"edge": [0, 30, 30]}));
    assert!(e.contains("print.unfold.edit"), "{e}");
    // Back to the edges: the sheets are gone, the solid shows, the bends are as they were.
    let v = run(&mut s, "print.unfold.edit", json!({}));
    assert_eq!((v["step"].as_str(), counts(&v)), (Some("edges"), (6, 5, 7, 0)), "{v}");
    assert_eq!(s.model.state().bodies.len(), 1);
    assert!(s.visibility.hidden_bodies.is_empty());
}

#[test]
fn what_cant_unfold_is_refused_and_says_why() {
    // A body with a curved face.
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 10, "height": 20, "body_name": "Rod"}));
    let undo = s.undo.len();
    let e = refused(&mut s, "print.unfold.start", json!({"body": "Rod", "face": [0, 0, 0]}));
    assert!(e.contains("curved faces"), "{e}");
    assert!(s.doc.print_unfold.is_none() && s.undo.len() == undo && s.model.state().body("Rod").is_some());
    // No body of that name, no face there, and a design that unfolds nothing.
    let e = refused(&mut s, "print.unfold.start", json!({"body": "Block", "face": [0, 0, 0]}));
    assert!(e.contains("Block"), "{e}");
    let e = refused(&mut s, "print.unfold.bend", json!({"edge": [0, 0, 0]}));
    assert!(e.contains("print.unfold.start"), "{e}");
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 100, "width": 60, "height": 30, "corner": [0, 0, 0], "body_name": "Block"}));
    let e = refused(&mut s, "print.unfold.start", json!({"body": "Block", "face": [50, 30, 15]}));
    assert!(e.contains("no face"), "{e}");
    let mut s = block();
    let e = refused(&mut s, "print.unfold.bend", json!({"edge": [0, 0, 15]}));
    assert!(e.contains("no edge"), "{e}");

    // An L-shaped bar: the edge in its inside corner can't fold, wherever the pattern has got to.
    let mut l = Session::default();
    run(&mut l, "sketch.create", json!({"plane": "XY", "name": "L"}));
    run(&mut l, "sketch.line", json!({"points": [[0, 0], [40, 0], [40, 10], [10, 10], [10, 30], [0, 30]], "closed": true}));
    run(&mut l, "sketch.finish", json!({}));
    run(&mut l, "solid.extrude", json!({"sketch": "L", "distance": 20, "body_name": "Bar"}));
    run(&mut l, "print.unfold.start", json!({"body": "Bar", "face": [5, 5, 0]}));
    let v = run(&mut l, "print.unfold.state", json!({}));
    // Eight faces and eighteen edges, one of them in the inside corner.
    assert_eq!(v["faces"], 8, "{v}");
    let all = || ["bends", "cuts", "open"].into_iter().flat_map(|k| v[k].as_array().into_iter().flatten());
    assert_eq!((all().count(), all().filter(|e| e["foldable"] == false).count()), (18, 1), "{v}");
    let inside = all().find(|e| e["foldable"] == false).cloned().unwrap_or_default();
    let mid: Vec<f64> = (0..3).map(|i| (inside["start"][i].as_f64().unwrap_or(0.0) + inside["end"][i].as_f64().unwrap_or(0.0)) / 2.0).collect();
    let e = refused(&mut l, "print.unfold.bend", json!({"edge": mid}));
    assert!(e.contains("inside corner"), "{e}");
}

fn mid(e: &Value) -> Vec<f64> {
    (0..3).map(|i| (e["start"][i].as_f64().unwrap_or(0.0) + e["end"][i].as_f64().unwrap_or(0.0)) / 2.0).collect()
}

/// An L-shaped bar (one inside corner), unfolded from its bottom with every face on the pattern
/// and laid flat.
fn l_bar_flat() -> Session {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "L"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [40, 0], [40, 10], [10, 10], [10, 30], [0, 30]], "closed": true}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "L", "distance": 20, "body_name": "Bar"}));
    run(&mut s, "print.unfold.start", json!({"body": "Bar", "face": [5, 5, 0]}));
    let v = run(&mut s, "print.unfold.state", json!({}));
    for e in v["cuts"].as_array().cloned().unwrap_or_default() {
        run(&mut s, "print.unfold.bend", json!({"edge": mid(&e)}));
    }
    // The top face hangs on a wall by one of that wall's top edges.
    let v = run(&mut s, "print.unfold.state", json!({}));
    let top = v["cuts"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|e| e["foldable"] == true && e["start"][2] == e["end"][2] && e["start"][2].as_f64().unwrap_or(0.0) > 10.0)
        .cloned();
    run(&mut s, "print.unfold.bend", json!({"edge": mid(&top.unwrap_or_else(|| panic!("no top edge")))}));
    let v = run(&mut s, "print.unfold.flatten", json!({}));
    assert_eq!((v["step"].as_str(), v["complete"].as_bool()), (Some("flat"), Some(true)), "{v}");
    s
}

#[test]
fn where_a_rigid_joint_cant_be_made_an_edge_gets_a_mitre_by_itself() {
    let mut s = l_bar_flat();
    // Film and butt only: no wall is needed beyond the film.
    let v = run(&mut s, "print.unfold.state", json!({}));
    assert_eq!(v["min_wall"].as_f64(), Some(0.0), "{v}");
    // Filament at every cut: the one inside corner gets a mitre, every other cut a filament joint.
    let v = run(&mut s, "print.unfold.joints", json!({"cuts": "filament"}));
    let joints: Vec<(String, bool)> =
        v["flat_cuts"].as_array().into_iter().flatten().map(|e| (e["joint"].as_str().unwrap_or("").to_string(), e["own"] == true)).collect();
    let (mitres, filaments) = (joints.iter().filter(|j| j.0 == "mitre").count(), joints.iter().filter(|j| j.0 == "filament").count());
    // The inside corner is one edge, listed once for each of its two faces.
    assert_eq!((mitres, filaments, joints.len()), (2, joints.len() - 2, joints.len()), "{v}");
    assert!(joints.iter().all(|j| !j.1), "nothing was set by hand: {joints:?}");
    // The least wall: the channel (1.75 + 0.1) and the film either side (0.4 each), up to 0.1.
    assert_eq!(v["min_wall"].as_f64(), Some(2.7), "{v}");
    let v = run(&mut s, "print.unfold.joints", json!({"cuts": "press"}));
    assert_eq!(v["min_wall"].as_f64(), Some(2.4), "{v}");
    // A joint of its own on that edge is refused, and says why; on another edge it is taken.
    let inside = v["flat_cuts"].as_array().into_iter().flatten().find(|e| e["joint"] == "mitre").cloned().unwrap_or_default();
    let e = refused(&mut s, "print.unfold.joint", json!({"edge": mid(&inside), "kind": "press"}));
    assert!(e.contains("inside corner"), "{e}");
    let other = v["flat_cuts"].as_array().into_iter().flatten().find(|e| e["joint"] == "press").cloned().unwrap_or_default();
    let v = run(&mut s, "print.unfold.joint", json!({"edge": mid(&other), "kind": "filament"}));
    assert_eq!(v["min_wall"].as_f64(), Some(2.7), "{v}");
    assert!(v["flat_cuts"].as_array().into_iter().flatten().any(|e| e["own"] == true && e["joint"] == "filament"), "{v}");
    // Bends that turn on the film alone have no wall to ask for.
    let v = run(&mut s, "print.unfold.joints", json!({"cuts": "butt", "bends": "film"}));
    assert_eq!(v["min_wall"].as_f64(), Some(2.7), "the edge of its own still needs its channel: {v}");
}

#[test]
fn the_walls_of_a_hole_are_carved_out_into_a_chain_that_folds_at_its_inside_corners() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Plate"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [40, 0], [40, 40], [0, 40]], "closed": true}));
    run(&mut s, "sketch.line", json!({"points": [[10, 10], [30, 10], [30, 30], [10, 30]], "closed": true}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "Plate", "distance": 10, "body_name": "Plate"}));
    // Lying on its base face, the body's centre of mass is over the origin.
    let v = run(&mut s, "print.unfold.start", json!({"body": "Plate", "face": [2, 2, 0]}));
    assert_eq!(v["faces"].as_u64(), Some(10), "{v}");
    // An inside corner of the hole can't fold before its walls are carved out.
    let why = refused(&mut s, "print.unfold.bend", json!({"edge": [10, 10, 5]}));
    assert!(why.contains("Carve its faces out"), "{why}");
    let v = run(&mut s, "print.unfold.group", json!({"faces": [[-10, 0, 5], [0, 10, 5], [10, 0, 5], [0, -10, 5]]}));
    assert_eq!((v["groups"].as_u64(), v["group_points"].as_array().map(Vec::len)), (Some(1), Some(1)), "{v}");
    // The base face and the group's first face are on the pattern.
    assert_eq!(v["attached"].as_u64(), Some(2), "{v}");
    // The border of the group stays a cut; three inside corners fold, the fourth would close the ring.
    let why = refused(&mut s, "print.unfold.bend", json!({"edge": [-10, 0, 0]}));
    assert!(why.contains("border of a carved-out group"), "{why}");
    for at in [[-10, 10, 5], [10, 10, 5], [10, -10, 5]] {
        run(&mut s, "print.unfold.bend", json!({"edge": at}));
    }
    let v = run(&mut s, "print.unfold.state", json!({}));
    assert_eq!((v["attached"].as_u64(), v["bends"].as_array().map(Vec::len)), (Some(5), Some(3)), "{v}");
    // The outer walls and the top, as on any body.
    for e in v["cuts"].as_array().cloned().unwrap_or_default() {
        if e["foldable"] == true {
            run(&mut s, "print.unfold.bend", json!({"edge": mid(&e)}));
        }
    }
    let v = run(&mut s, "print.unfold.state", json!({}));
    let top = v["cuts"].as_array().into_iter().flatten().find(|e| e["foldable"] == true && e["start"][2] == e["end"][2]).cloned();
    run(&mut s, "print.unfold.bend", json!({"edge": mid(&top.unwrap_or_else(|| panic!("no top edge: {v}")))}));
    let v = run(&mut s, "print.unfold.flatten", json!({}));
    assert_eq!((v["step"].as_str(), v["complete"].as_bool()), (Some("flat"), Some(true)), "{v}");
    // The chain is a part of its own, its four walls in a row: 4 x 20 long, 10 high.
    let parts = v["parts"].as_array().cloned().unwrap_or_default();
    assert_eq!(parts.len(), 2, "{v}");
    let chain = parts.iter().find(|p| p["faces"] == 4).unwrap_or_else(|| panic!("no chain: {v}"));
    assert!((chain["long"].as_f64().unwrap_or(0.0) - 80.0).abs() < 1e-6 && (chain["short"].as_f64().unwrap_or(0.0) - 10.0).abs() < 1e-6, "{chain}");
    // Made: the body, and the chain beside it as a body of its own, its walls 2 mm thick and
    // 2.15 mm shorter at the top and the bottom (the plate's wall and the clearance).
    let v = run(&mut s, "print.unfold.make", json!({}));
    assert_eq!(v["step"].as_str(), Some("solid"), "{v}");
    let (lo, hi) = bbox(&mut s, "Plate Group 1");
    assert!((hi[2] - lo[2] - 2.0).abs() < 1e-6, "{lo:?} {hi:?}");
    let (long, short) = ((hi[0] - lo[0]).max(hi[1] - lo[1]), (hi[0] - lo[0]).min(hi[1] - lo[1]));
    assert!((long - 80.0).abs() < 1e-6 && (short - (10.0 - 2.0 * 2.15)).abs() < 1e-6, "{long} {short}");
    let v = run(&mut s, "print.unfold.unmake", json!({}));
    assert_eq!(v["step"].as_str(), Some("flat"), "{v}");
    // With a press fit at the cuts the chain's fingers stand through the plate's two faces, in
    // sockets there: the chain is as high as the plate again, and the plate lost the notches.
    let v = run(&mut s, "print.unfold.joints", json!({"wall": "3 mm", "cuts": "press"}));
    let border = v["flat_cuts"].as_array().into_iter().flatten().filter(|e| e["joint"] == "press").count();
    assert!(border >= 16, "the eight border edges of the chain, each on two faces: {v}");
    run(&mut s, "print.unfold.make", json!({}));
    let (lo, hi) = bbox(&mut s, "Plate Group 1");
    let short = (hi[0] - lo[0]).min(hi[1] - lo[1]);
    assert!((short - 10.0).abs() < 1e-6 && (hi[2] - lo[2] - 3.0).abs() < 1e-6, "{lo:?} {hi:?}");
    // A mitre instead: the chain ends in a slope at the plate's outer faces.
    run(&mut s, "print.unfold.unmake", json!({}));
    run(&mut s, "print.unfold.joints", json!({"cuts": "mitre"}));
    run(&mut s, "print.unfold.make", json!({}));
    let (lo, hi) = bbox(&mut s, "Plate Group 1");
    let short = (hi[0] - lo[0]).min(hi[1] - lo[1]);
    assert!(short > 9.0 && short < 10.0, "{lo:?} {hi:?}");
}

#[test]
fn generator_2_makes_the_bodys_own_walls_mitred_and_joined_by_the_film() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 100, "width": 60, "height": 30, "corner": [0, 0, 0], "body_name": "Block"}));
    let why = refused(&mut s, "print.unfold.start", json!({"body": "Block", "face": [50, 30, 0], "generator": 3}));
    assert!(why.contains("must be 1 or 2"), "{why}");
    let v = run(&mut s, "print.unfold.start", json!({"body": "Block", "face": [50, 30, 0], "generator": 2}));
    assert_eq!(v["generator"].as_u64(), Some(2), "{v}");
    // The four walls on the bottom face, the top on a wall.
    for e in v["cuts"].as_array().cloned().unwrap_or_default() {
        run(&mut s, "print.unfold.bend", json!({"edge": mid(&e)}));
    }
    let v = run(&mut s, "print.unfold.state", json!({}));
    let top =
        v["cuts"].as_array().into_iter().flatten().find(|e| e["start"][2] == e["end"][2] && e["start"][2].as_f64().unwrap_or(0.0) > 10.0).cloned();
    run(&mut s, "print.unfold.bend", json!({"edge": mid(&top.unwrap_or_else(|| panic!("no top edge")))}));
    let v = run(&mut s, "print.unfold.flatten", json!({}));
    assert_eq!((v["step"].as_str(), v["complete"].as_bool()), (Some("flat"), Some(true)), "{v}");
    // Every cut is mitred, every bend turns on the film, and no wall is asked for beyond the film.
    assert!(v["flat_cuts"].as_array().into_iter().flatten().all(|e| e["joint"] == "mitre"), "{v}");
    assert!(v["flat_bends"].as_array().into_iter().flatten().all(|e| e["joint"] == "film"), "{v}");
    assert_eq!(v["min_wall"].as_f64(), Some(0.0), "{v}");
    let why = refused(&mut s, "print.unfold.joint", json!({"edge": mid(&v["flat_bends"][0]), "kind": "press"}));
    assert!(why.contains("a bend turns on the film"), "{why}");
    let why = refused(&mut s, "print.unfold.joint", json!({"edge": mid(&v["flat_cuts"][0]), "kind": "filament"}));
    assert!(why.contains("no filament joints"), "{why}");
    let v = run(&mut s, "print.unfold.make", json!({}));
    assert_eq!(v["step"].as_str(), Some("solid"), "{v}");
    // One part, lying flat, as thick as the wall: one piece, joined by the film under its bends.
    let (lo, hi) = bbox(&mut s, "Block Part 1");
    assert!((hi[2] - lo[2] - 2.0).abs() < 1e-6, "{lo:?} {hi:?}");
    {
        let world = s.world_state().unwrap_or_else(|e| panic!("{e}"));
        let part = &world.body("Block Part 1").unwrap_or_else(|| panic!("no part")).body;
        assert_eq!(part.lumps().map(|l| l.len()).unwrap_or(0), 1, "the film joins the walls into one piece");
        // The walls are the block's faces 2 mm deep, 43200 mm³, less what they share behind
        // its twelve edges (760 mm of them, 4 mm² each: 3040 mm³). The clearance takes a little
        // more and the film adds a little: the part is the block's shell and no more.
        let volume = solidcraft_kernel::measure(part).unwrap_or_else(|e| panic!("{e}")).volume;
        assert!(volume > 39_700.0 && volume < 40_200.0, "{volume}");
    }
    let v = run(&mut s, "print.unfold.unmake", json!({}));
    assert_eq!(v["step"].as_str(), Some("flat"), "{v}");
}

/// A block `height` high laid flat by generator 2: its four walls on the bottom face, the top
/// on a wall. The state once flat.
fn generator_2_block(s: &mut Session, height: f64) -> Value {
    run(s, "solid.box", json!({"length": 100, "width": 60, "height": height, "corner": [0, 0, 0], "body_name": "Block"}));
    let v = run(s, "print.unfold.start", json!({"body": "Block", "face": [50, 30, 0], "generator": 2}));
    for e in v["cuts"].as_array().cloned().unwrap_or_default() {
        run(s, "print.unfold.bend", json!({"edge": mid(&e)}));
    }
    let v = run(s, "print.unfold.state", json!({}));
    let top =
        v["cuts"].as_array().into_iter().flatten().find(|e| e["start"][2] == e["end"][2] && e["start"][2].as_f64().unwrap_or(0.0) > 1.0).cloned();
    run(s, "print.unfold.bend", json!({"edge": mid(&top.unwrap_or_else(|| panic!("no top edge")))}));
    run(s, "print.unfold.flatten", json!({}))
}

#[test]
fn generator_2_press_fits_a_cut_with_fingers_and_leaves_its_bends_alone() {
    let mut s = Session::default();
    let v = generator_2_block(&mut s, 30.0);
    // Every cut lies in the flat twice, once on each of its faces.
    let cuts = v["flat_cuts"].as_array().cloned().unwrap_or_default();
    let edges = |v: &Value, kind: &str| -> std::collections::BTreeSet<u64> {
        v["flat_cuts"].as_array().into_iter().flatten().filter(|e| e["joint"] == kind).filter_map(|e| e["edge"].as_u64()).collect()
    };
    assert_eq!(edges(&v, "mitre").len(), 7, "{v}");
    // One cut of its own, then every cut: all of them are square and long enough.
    let v = run(&mut s, "print.unfold.joint", json!({"edge": mid(&cuts[0]), "kind": "press"}));
    assert_eq!(edges(&v, "press").len(), 1, "{v}");
    run(&mut s, "print.unfold.joint", json!({"edge": mid(&cuts[0]), "kind": "default"}));
    let v = run(&mut s, "print.unfold.joints", json!({"cuts": "press"}));
    assert_eq!((edges(&v, "press").len(), edges(&v, "mitre").len()), (7, 0), "{v}");
    assert!(v["flat_bends"].as_array().into_iter().flatten().all(|e| e["joint"] == "film"), "{v}");
    assert_eq!(v["min_wall"].as_f64(), Some(1.2), "{v}");
    run(&mut s, "print.unfold.make", json!({}));
    let (lo, hi) = bbox(&mut s, "Block Part 1");
    assert!((hi[2] - lo[2] - 2.0).abs() < 1e-6, "the fingers and their nubs stay in the wall: {lo:?} {hi:?}");
    {
        let world = s.world_state().unwrap_or_else(|e| panic!("{e}"));
        let part = &world.body("Block Part 1").unwrap_or_else(|| panic!("no part")).body;
        assert_eq!(part.lumps().map(|l| l.len()).unwrap_or(0), 1, "no fragment lies loose beside the part");
        // The fingers deal the shared corners out anew, they add nothing: the shell's volume
        // as with mitres, a little less for the clearance round every finger.
        let volume = solidcraft_kernel::measure(part).unwrap_or_else(|e| panic!("{e}")).volume;
        assert!(volume > 39_500.0 && volume < 40_200.0, "{volume}");
    }
    // A block too low for a finger clear of the corners: its upright cuts stay mitred.
    let mut s = Session::default();
    generator_2_block(&mut s, 8.0);
    let v = run(&mut s, "print.unfold.joints", json!({"cuts": "press"}));
    assert_eq!((edges(&v, "mitre").len(), edges(&v, "press").len()), (4, 3), "{v}");
    let short = v["flat_cuts"].as_array().into_iter().flatten().find(|e| e["joint"] == "mitre").cloned().unwrap_or_else(|| panic!("no mitred cut"));
    let why = refused(&mut s, "print.unfold.joint", json!({"edge": mid(&short), "kind": "press"}));
    assert!(why.contains("can't be a press fit"), "{why}");
    run(&mut s, "print.unfold.make", json!({}));
}

#[test]
fn generator_2_keeps_fingers_off_a_cut_another_wall_lies_beside_in_the_flat() {
    // An L 20 mm high. Its walls fold out from the bottom face, all but one of the two at the
    // inside corner of the L: that one hangs on the top face, which hangs on a wall.
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "L"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [40, 0], [40, 20], [20, 20], [20, 40], [0, 40]], "closed": true}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "L", "distance": 20, "body_name": "L"}));
    let v = run(&mut s, "print.unfold.start", json!({"body": "L", "face": [10, 10, 0], "generator": 2}));
    // Where the L's corner at the origin lies in the copy being unfolded.
    let least = |i: usize| {
        v["cuts"]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|e| [e["start"][i].as_f64(), e["end"][i].as_f64()])
            .flatten()
            .fold(f64::INFINITY, f64::min)
    };
    let (ox, oy) = (least(0), least(1));
    for (x, y, z) in
        [(20.0, 0.0, 0.0), (40.0, 10.0, 0.0), (30.0, 20.0, 0.0), (10.0, 40.0, 0.0), (0.0, 20.0, 0.0), (20.0, 0.0, 20.0), (20.0, 30.0, 20.0)]
    {
        run(&mut s, "print.unfold.bend", json!({"edge": [x + ox, y + oy, z]}));
    }
    let v = run(&mut s, "print.unfold.flatten", json!({}));
    assert_eq!((v["step"].as_str(), v["complete"].as_bool()), (Some("flat"), Some(true)), "{v}");
    let v = run(&mut s, "print.unfold.joints", json!({"cuts": "press"}));
    let edges = |kind: &str| -> std::collections::BTreeSet<u64> {
        v["flat_cuts"].as_array().into_iter().flatten().filter(|e| e["joint"] == kind).filter_map(|e| e["edge"].as_u64()).collect()
    };
    // Mitred all the same: the upright cut in the inside corner, and the two cuts a wall folded
    // out at that corner lies beside (the bottom's and the top's). A finger there would print
    // as one with that wall.
    assert_eq!((edges("press").len(), edges("mitre").len()), (8, 3), "{v}");
    for e in v["flat_cuts"].as_array().into_iter().flatten().filter(|e| e["joint"] == "mitre") {
        let why = refused(&mut s, "print.unfold.joint", json!({"edge": mid(e), "kind": "press"}));
        assert!(why.contains("can't be a press fit"), "{why}");
    }
    run(&mut s, "print.unfold.make", json!({}));
    let world = s.world_state().unwrap_or_else(|e| panic!("{e}"));
    let part = &world.body("L Part 1").unwrap_or_else(|| panic!("no part")).body;
    assert_eq!(part.lumps().map(|l| l.len()).unwrap_or(0), 1, "no fragment lies loose beside the part");
}

#[test]
fn a_box_with_flat_chamfers_unfolds_as_any_body_of_flat_faces() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Box"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [40, 0], [40, 30], [0, 30]], "closed": true}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "Box", "distance": 20, "body_name": "Box"}));
    run(&mut s, "solid.chamfer", json!({"edges": [[20, 0, 20], [0, 15, 20]], "distance": 4, "body": "Box"}));
    let v = run(&mut s, "print.unfold.start", json!({"body": "Box", "face": [20, 15, 0]}));
    assert_eq!(v["faces"].as_u64(), Some(8), "{v}");
}
