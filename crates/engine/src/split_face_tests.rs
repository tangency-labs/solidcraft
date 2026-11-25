//! Split Face: the top of a box divided by a plane, by another body's face and by sketch curves.

use serde_json::{Value, json};

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

/// (kernel faces, volume) of a body: the merged count joins pieces of one surface back up.
fn faces_volume(s: &mut Session, b: &str) -> (u64, f64) {
    let m = run(s, "inspect.measure", json!({ "bodies": [b] }))["bodies"][0].clone();
    (m["kernel"]["faces"].as_u64().unwrap_or(0), m["volume_mm3"].as_f64().unwrap_or(0.0))
}

fn boxed() -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 20, "body_name": "B"}));
    s
}

#[test]
fn split_a_face_by_a_plane() {
    let mut s = boxed();
    run(&mut s, "solid.split_face", json!({"faces": [[10, 15, 20]], "plane": {"origin": [20, 0, 0], "normal": [1, 0, 0]}}));
    let (f, v) = faces_volume(&mut s, "B");
    assert_eq!(f, 7);
    assert!((v - 24000.0).abs() < 1e-6 * 24000.0, "{v}");
    assert_eq!(s.doc.features.last().unwrap().kind.type_name(), "SplitFaceFeature");
}

#[test]
fn split_a_face_by_another_bodys_face() {
    let mut s = boxed();
    run(&mut s, "solid.box", json!({"corner": [-5, 10, 25], "length": 50, "width": 5, "height": 5, "body_name": "Tool"}));
    // The tool's face at y = 10, extended down through the box's top.
    run(&mut s, "solid.split_face", json!({"faces": [[20, 5, 20]], "body": "B", "tool": {"body": "Tool", "point": [20, 10, 27]}}));
    assert_eq!(faces_volume(&mut s, "B").0, 7);
}

#[test]
fn split_a_face_by_sketch_curves() {
    let mut s = boxed();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Cuts"}));
    run(&mut s, "sketch.line", json!({"points": [[10, -5], [10, 35]]}));
    run(&mut s, "sketch.line", json!({"points": [[30, -5], [30, 35]]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.split_face", json!({"faces": [[20, 15, 20]], "body": "B", "sketch": "Cuts"}));
    let (f, v) = faces_volume(&mut s, "B");
    assert_eq!(f, 8, "two lines make three pieces of the top");
    assert!((v - 24000.0).abs() < 1e-6 * 24000.0, "{v}");
}

/// A triangle of three lines on the top: by default each side is extended across the face; with
/// `loops` the sides are joined first and the closed loop cuts out the triangle only. Open curves
/// are extended either way.
#[test]
fn split_a_face_by_a_closed_loop_of_lines() {
    let split = |loops: bool| {
        let mut s = boxed();
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Loop"}));
        run(&mut s, "sketch.line", json!({"points": [[10, 5], [30, 5], [20, 25]], "closed": true}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.split_face", json!({"faces": [[20, 12, 20]], "body": "B", "sketch": "Loop", "loops": loops}));
        faces_volume(&mut s, "B")
    };
    let (f, v) = split(true);
    assert_eq!(f, 7, "the triangle and the rest of the top");
    assert!((v - 24000.0).abs() < 1e-6 * 24000.0, "{v}");
    assert!(split(false).0 > 7, "each side extended across the top");

    let mut s = boxed();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Cuts"}));
    run(&mut s, "sketch.line", json!({"points": [[10, 10], [10, 20]]}));
    run(&mut s, "sketch.line", json!({"points": [[30, 10], [30, 20]]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.split_face", json!({"faces": [[20, 15, 20]], "body": "B", "sketch": "Cuts", "loops": true}));
    assert_eq!(faces_volume(&mut s, "B").0, 8, "two open lines still cut across the top");
}

#[test]
fn a_tool_that_misses_is_refused() {
    let mut s = boxed();
    let n = s.doc.features.len();
    assert!(s.execute("solid.split_face", &json!({"faces": [[10, 15, 20]], "plane": {"origin": [100, 0, 0], "normal": [1, 0, 0]}})).is_err());
    assert_eq!(s.doc.features.len(), n);
}

#[test]
fn export_keeps_split_faces_through_later_features() {
    let mut s = boxed();
    run(&mut s, "solid.split_face", json!({"faces": [[10, 15, 20]], "plane": {"origin": [20, 0, 0], "normal": [1, 0, 0]}}));
    // A later feature rebuilds the body; the split stays a split in the file.
    run(&mut s, "solid.fillet", json!({"edges": [[20, 0, 0]], "radius": 2}));
    let (f, _) = faces_volume(&mut s, "B");
    assert_eq!(f, 8);
    let dir = std::env::temp_dir().join(format!("solidcraft-split-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let step = dir.join("split.step");
    run(&mut s, "file.export", json!({"path": step.to_string_lossy()}));
    let text = std::fs::read_to_string(&step).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    // Six box faces, the top's second piece and the fillet.
    assert_eq!(text.matches("ADVANCED_FACE(").count(), 8, "{text}");
}

#[test]
fn split_pieces_count_and_move_apart() {
    let mut s = boxed();
    run(&mut s, "solid.split_face", json!({"faces": [[10, 15, 20]], "plane": {"origin": [20, 0, 0], "normal": [1, 0, 0]}}));
    // Measure counts the two halves of the top as two faces.
    let m = run(&mut s, "inspect.measure", json!({ "bodies": ["B"] }))["bodies"][0].clone();
    assert_eq!(m["faces"].as_u64(), Some(7), "{m}");
    // Offset Face on one half moves that half only: a 20 × 30 step 5 high.
    run(&mut s, "solid.offset_face", json!({"faces": [[10, 15, 20]], "distance": 5}));
    let (_, v) = faces_volume(&mut s, "B");
    assert!((v - 27000.0).abs() < 1e-6 * 27000.0, "{v}");
    // And the other half down 3: a 20 × 30 pocket.
    run(&mut s, "solid.offset_face", json!({"faces": [[30, 15, 20]], "distance": -3}));
    let (_, v) = faces_volume(&mut s, "B");
    assert!((v - 25200.0).abs() < 1e-6 * 25200.0, "{v}");
}

/// The face of body `b` at `p`, its index and its name.
fn face_at(s: &Session, b: &str, p: [f64; 3]) -> (usize, String) {
    let st = s.model.state();
    let body = st.body(b).unwrap();
    let i = solidcraft_doc::appearance::face_index_at(body, solidcore_record::Vec3::new(p[0], p[1], p[2])).unwrap();
    (i, solidcraft_doc::naming::face_names(body)[i].clone())
}

/// The faces of body `b` with a look of their own, and the colour.
fn coloured(s: &Session, b: &str) -> Vec<(usize, [u8; 3])> {
    let st = s.model.state();
    let mut out: Vec<(usize, [u8; 3])> = s.doc.face_colors(st.body(b).unwrap()).into_iter().map(|(i, l)| (i, l.color)).collect();
    out.sort();
    out
}

/// The box's top split by a circle drawn on it (`r` a parameter): a disc and the ring around it.
fn disc_in_top() -> Session {
    let mut s = boxed();
    run(&mut s, "parameters.add", json!({"name": "r", "expression": "5 mm"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Ring"}));
    run(&mut s, "sketch.circle.center", json!({"center": [20, 15], "radius": 5}));
    run(&mut s, "sketch.dimension", json!({"entities": ["c1"], "type": "radius", "value": "r"}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.split_face", json!({"faces": [[5, 5, 20]], "body": "B", "sketch": "Ring", "loops": true, "name": "Cut"}));
    s
}

/// Split Face names its pieces itself, as Extrude names its faces: the face they came from and
/// the feature with the piece's side of its tool. A disc cut out of the top is the top's piece
/// inside the loop, the ring the piece outside, neither the top's own name.
#[test]
fn split_face_names_its_pieces_by_their_side_of_the_tool() {
    let s = disc_in_top();
    let split = s.doc.find_feature("Cut").unwrap().id;
    let top = solidcraft_doc::naming::strip_piece(&face_at(&s, "B", [20., 15., 20.]).1).to_string();
    assert_eq!(face_at(&s, "B", [20., 15., 20.]).1, format!("{top}#[F{split}:split:in]"));
    assert_eq!(face_at(&s, "B", [5., 5., 20.]).1, format!("{top}#[F{split}:split:out]"));

    // A plane: the side its normal points to and the other.
    let mut s = boxed();
    run(&mut s, "solid.split_face", json!({"faces": [[10, 15, 20]], "plane": {"origin": [15, 0, 0], "normal": [1, 0, 0]}, "name": "Half"}));
    let split = s.doc.find_feature("Half").unwrap().id;
    let (_, low) = face_at(&s, "B", [5., 15., 20.]);
    let (_, high) = face_at(&s, "B", [30., 15., 20.]);
    assert!(low.ends_with(&format!("#[F{split}:split:-]")), "{low}");
    assert!(high.ends_with(&format!("#[F{split}:split:+]")), "{high}");
}

/// The disc alone takes a colour given to the disc (the ring keeps none), also once Offset Face
/// has raised it; a colour given to the top before the split stays on every piece.
#[test]
fn a_colour_on_a_split_piece_colours_that_piece_alone() {
    let mut s = disc_in_top();
    run(&mut s, "appearance.assign", json!({"body": "B", "faces": [[20, 15, 20]], "color": "#ff0000"}));
    let (disc, _) = face_at(&s, "B", [20., 15., 20.]);
    assert_eq!(coloured(&s, "B"), vec![(disc, [255, 0, 0])]);

    let mut s = disc_in_top();
    run(&mut s, "solid.offset_face", json!({"faces": [[20, 15, 20]], "distance": 2}));
    run(&mut s, "appearance.assign", json!({"body": "B", "faces": [[20, 15, 22]], "color": "#ff0000"}));
    let (disc, _) = face_at(&s, "B", [20., 15., 22.]);
    assert_eq!(coloured(&s, "B"), vec![(disc, [255, 0, 0])]);

    // Coloured before the split: the top's name, so every piece of it.
    let mut s = boxed();
    run(&mut s, "appearance.assign", json!({"body": "B", "faces": [[5, 5, 20]], "color": "#00ff00"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Ring"}));
    run(&mut s, "sketch.circle.center", json!({"center": [20, 15], "radius": 5}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.split_face", json!({"faces": [[5, 5, 20]], "body": "B", "sketch": "Ring", "loops": true}));
    let (disc, _) = face_at(&s, "B", [20., 15., 20.]);
    let (ring, _) = face_at(&s, "B", [5., 5., 20.]);
    let mut both = vec![(disc, [0, 255, 0]), (ring, [0, 255, 0])];
    both.sort();
    assert_eq!(coloured(&s, "B"), both);
}

/// Upstream edits that move and resize the pieces keep their names: a bigger circle, a longer box.
#[test]
fn split_piece_names_survive_upstream_edits() {
    let mut s = disc_in_top();
    let names = |s: &Session| (face_at(s, "B", [20., 15., 20.]).1, face_at(s, "B", [2., 2., 20.]).1);
    let before = names(&s);
    run(&mut s, "parameters.change", json!({"name": "r", "expression": "8 mm"}));
    assert_eq!(names(&s), before);
    run(&mut s, "appearance.assign", json!({"body": "B", "faces": [[20, 15, 20]], "color": "#ff0000"}));
    run(&mut s, "parameters.change", json!({"name": "r", "expression": "4 mm"}));
    let (disc, _) = face_at(&s, "B", [20., 15., 20.]);
    assert_eq!(coloured(&s, "B"), vec![(disc, [255, 0, 0])], "the colour follows the disc");
}

/// A design saved before Split Face named its pieces holds the names the faces around them gave
/// them: the disc's the top's bare name, the ring's the top qualified by the box's sides. Those
/// still find the same pieces, so a later feature that picked one replays as it did.
#[test]
fn a_design_saved_before_finds_split_pieces_by_their_former_names() {
    let mut s = disc_in_top();
    run(&mut s, "solid.offset_face", json!({"faces": [[20, 15, 20]], "distance": 2, "name": "Raise"}));
    let raised = faces_volume(&mut s, "B").1;
    assert!((raised - (24000.0 + std::f64::consts::PI * 25.0 * 2.0)).abs() < 1e-6 * raised, "{raised}");
    let (disc, name) = face_at(&s, "B", [20., 15., 22.]);
    let top = solidcraft_doc::naming::strip_piece(&name).to_string();
    let k = s.doc.features.iter().position(|f| f.name == "Raise").unwrap();
    // As an older build saved the Offset Face's pick: the disc by the top's bare name.
    let mut old: solidcraft_doc::Document = (*s.doc).clone();
    old.features[k].face_names = vec![top.clone()];
    let mut t = Session::new(solidcraft_doc::Document::from_json(&old.to_json()).unwrap());
    t.refresh();
    assert!(t.model.results.iter().all(|r| r.error.is_none()), "{:?}", t.model.results);
    assert!((faces_volume(&mut t, "B").1 - raised).abs() < 1e-9 * raised);
    // And the ring by the box's sides around it.
    let st = s.model.state();
    let (ring, _) = face_at(&s, "B", [2., 2., 20.]);
    let mut sides: Vec<String> = [[0., 15., 10.], [40., 15., 10.], [20., 0., 10.], [20., 30., 10.]].iter().map(|p| face_at(&s, "B", *p).1).collect();
    sides.sort();
    let ring_old = format!("{top}#[{}]", sides.join(","));
    let at = |n: &str, p: [f64; 3]| solidcraft_doc::naming::locate_face(&st, n, solidcore_record::Vec3::new(p[0], p[1], p[2])).map(|h| h.face);
    assert_eq!(at(&ring_old, [2., 2., 20.]), Some(ring));
    assert_eq!(at(&top, [20., 15., 22.]), Some(disc));
}
