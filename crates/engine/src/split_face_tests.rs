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

// Split Face by 3D curves (#22414), on the scratch sheet: a circular arc 40 mm wide with an 8 mm
// rise (radius 29 mm, centre (20, -21)) in the XY plane, extruded 30 mm along Z. Its area is
// 29 · 2 asin(20/29) · 30 = 1324.17 mm². Face areas come from meshes, so they are checked to 0.1 %.

const R: f64 = 29.0;

fn half() -> f64 {
    (20.0 / R).asin()
}

/// The whole sheet's area.
fn sheet_area() -> f64 {
    R * 2.0 * half() * 30.0
}

/// The sheet, and a sketch `Marks` with 3D Sketch on, left open for the curves.
fn sheet() -> Session {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Arc"}));
    run(&mut s, "sketch.arc.three_point", json!({"start": [0, 0], "end": [40, 0], "through": [20, 8]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "surface.extrude", json!({"sketch": "Arc", "curves": ["a1"], "distance": 30, "body_name": "Sheet"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Marks"}));
    run(&mut s, "sketch.options", json!({"sketch_3d": true}));
    s
}

/// The areas of the sheet's faces, smallest first.
fn sheet_faces(s: &mut Session) -> Vec<f64> {
    let f = run(s, "model.faces", json!({"body": "Sheet"}));
    let mut a: Vec<f64> = f["faces"].as_array().cloned().unwrap_or_default().iter().map(|x| x["area"].as_f64().unwrap_or(f64::NAN)).collect();
    a.sort_by(f64::total_cmp);
    a
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-3 * b.abs()
}

/// The sheet's height at `x`: y = sqrt(29² − (x − 20)²) − 21.
fn height(x: f64) -> f64 {
    (R * R - (x - 20.0) * (x - 20.0)).sqrt() - 21.0
}

/// A straight 3D line lying on the sheet along its length (x = 30), from edge to edge, swept along
/// the sheet's normal there (the default): it cuts along itself. The piece past it spans
/// asin(10/29) to asin(20/29) of the arc: 29 · (asin(20/29) − asin(10/29)) · 30 = 355.82 mm².
#[test]
fn split_face_by_a_3d_line_lying_on_a_curved_sheet() {
    let mut s = sheet();
    run(&mut s, "sketch.line3d", json!({"points": [[30.0, height(30.0), 0.0], [30.0, height(30.0), 30.0]]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.split_face", json!({"faces": [[20, 8, 15]], "body": "Sheet", "sketch": "Marks"}));
    let a = sheet_faces(&mut s);
    let side = R * (half() - (10.0 / R).asin()) * 30.0;
    assert_eq!(a.len(), 2, "{a:?}");
    assert!(close(a[0], side) && close(a[1], sheet_area() - side), "{a:?} against {side} and {}", sheet_area() - side);
    assert!(close(a[0] + a[1], sheet_area()));
}

/// A 3D control point spline across the sheet from edge to edge (control points (0, 0, 10),
/// (20, 16, 0), (40, 0, 10): a quadratic whose plan view along Y is z = 5 + (x − 20)²/80), swept
/// along +Y. The piece on the z = 0 side is ∫ (5 + u²/80) · 29 / sqrt(29² − u²) du over -20..20
/// = 29 · (10 asin(20/29) + (29² asin(20/29) − 20 · 21) / 80) = 300.45 mm².
#[test]
fn split_face_by_a_3d_spline_across_a_curved_sheet() {
    let mut s = sheet();
    let w = run(&mut s, "sketch.spline3d.control_point", json!({"points": [[0, 0, 10], [20, 16, 0], [40, 0, 10]]}))["curves"][0]
        .as_str()
        .unwrap()
        .to_string();
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.split_face", json!({"faces": [[20, 8, 15]], "body": "Sheet", "sketch": "Marks", "wires": [w], "direction": [0, 1, 0]}));
    let a = sheet_faces(&mut s);
    let near = R * (10.0 * half() + (R * R * half() - 20.0 * 21.0) / 80.0);
    assert_eq!(a.len(), 2, "{a:?}");
    // The mesh of a face trimmed by a free-form edge sags more than one trimmed by straight
    // lines (measured 300.80 and 1022.86): 0.2 % here. The kernel's own test checks the exact
    // areas to a millionth (crates/kernel/src/split_curves3d_tests.rs).
    let near_enough = |a: f64, b: f64| (a - b).abs() <= 2e-3 * b;
    assert!(near_enough(a[0], near) && near_enough(a[1], sheet_area() - near), "{a:?} against {near} and {}", sheet_area() - near);
    assert!(close(a[0] + a[1], sheet_area()));
    let solidcraft_doc::FeatureKind::SplitFace { wires, direction, .. } = &s.doc.features.last().unwrap().kind else { panic!("a Split Face") };
    assert_eq!((wires.clone(), direction.map(|d| d.0)), (vec![w], Some(solidcore_record::Vec3::Y)));
}

/// The sheet's own cross-section at z = 15 as a 3D arc from edge to edge: swept along the mean
/// normal of the sheet at it (the default, in the arc's plane), it halves the sheet.
#[test]
fn split_face_by_a_3d_arc_halves_a_curved_sheet() {
    let mut s = sheet();
    run(&mut s, "sketch.arc3d", json!({"start": [0, 0, 15], "through": [20, 8, 15], "end": [40, 0, 15]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.split_face", json!({"faces": [[20, 8, 5]], "body": "Sheet", "sketch": "Marks"}));
    let a = sheet_faces(&mut s);
    assert_eq!(a.len(), 2, "{a:?}");
    assert!(close(a[0], sheet_area() / 2.0) && close(a[1], sheet_area() / 2.0), "{a:?}");
}

/// Refused: a direction along the face all the way along the curve (the sheet would touch the
/// face, not cross it), a zero direction, a curve id the sketch does not have, and a curve whose
/// sheet misses the face. Nothing is added.
#[test]
fn split_face_by_3d_curves_refuses_a_tangent_sweep_and_a_miss() {
    let mut s = boxed();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Marks"}));
    run(&mut s, "sketch.options", json!({"sketch_3d": true}));
    run(&mut s, "sketch.line3d", json!({"points": [[5, 15, 20], [35, 15, 20]]}));
    run(&mut s, "sketch.finish", json!({}));
    let n = s.doc.features.len();
    let tangent = s.execute("solid.split_face", &json!({"faces": [[20, 15, 20]], "body": "B", "sketch": "Marks", "direction": [0, 1, 0]}));
    assert!(tangent.as_ref().is_err_and(|e| e.to_string().contains("tangent")), "{tangent:?}");
    let zero = s.execute("solid.split_face", &json!({"faces": [[20, 15, 20]], "body": "B", "sketch": "Marks", "direction": [0, 0, 0]}));
    assert!(zero.as_ref().is_err_and(|e| e.to_string().contains("zero")), "{zero:?}");
    let unknown = s.execute("solid.split_face", &json!({"faces": [[20, 15, 20]], "body": "B", "sketch": "Marks", "wires": ["w99"]}));
    assert!(unknown.as_ref().is_err_and(|e| e.to_string().contains("w99")), "{unknown:?}");
    assert_eq!(s.doc.features.len(), n);
    // Along the top's normal (the default) the line cuts the top in two.
    run(&mut s, "solid.split_face", json!({"faces": [[20, 10, 20]], "body": "B", "sketch": "Marks"}));
    assert_eq!(faces_volume(&mut s, "B").0, 7);

    let mut s = sheet();
    run(&mut s, "sketch.line3d", json!({"points": [[50, 0, 0], [50, 0, 30]]}));
    run(&mut s, "sketch.finish", json!({}));
    // Beside the sheet it lies over none of it, so there is no normal to default to; along +Y its
    // sheet stands beside the face and misses it.
    let n = s.doc.features.len();
    let default = s.execute("solid.split_face", &json!({"faces": [[20, 8, 15]], "body": "Sheet", "sketch": "Marks"}));
    assert!(default.as_ref().is_err_and(|e| e.to_string().contains("give a direction")), "{default:?}");
    let miss = s.execute("solid.split_face", &json!({"faces": [[20, 8, 15]], "body": "Sheet", "sketch": "Marks", "direction": [0, 1, 0]}));
    assert!(miss.as_ref().is_err_and(|e| e.to_string().contains("does not cross")), "{miss:?}");
    assert_eq!(s.doc.features.len(), n);
}

/// A body placed 100 mm along +X (its own offset): a 3D line drawn where the body is shown, on its
/// top at world x = 110 (x = 10 in the body's own frame), cuts the 40 × 30 top into 10 · 30 = 300
/// and 30 · 30 = 900 mm². The curve is read in the body's stored frame, not at its world x = 110.
#[test]
fn split_face_by_a_3d_line_on_a_placed_body() {
    use solidcore_record::Vec3;
    let mut s = boxed();
    s.doc_mut().body_offsets.insert("B".into(), solidcraft_doc::rigid(Vec3::new(100.0, 0.0, 0.0), Vec3::ZERO, Vec3::Z, 0.0));
    s.refresh();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Marks"}));
    run(&mut s, "sketch.options", json!({"sketch_3d": true}));
    run(&mut s, "sketch.line3d", json!({"points": [[110, 5, 20], [110, 25, 20]]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.split_face", json!({"faces": [[20, 15, 20]], "body": "B", "sketch": "Marks"}));
    assert_eq!(faces_volume(&mut s, "B").0, 7);
    let f = run(&mut s, "model.faces", json!({"body": "B"}));
    let mut a: Vec<f64> = f["faces"].as_array().cloned().unwrap_or_default().iter().map(|x| x["area"].as_f64().unwrap_or(f64::NAN)).collect();
    a.sort_by(f64::total_cmp);
    // Sides 600, 600, 800, 800, bottom 1200, and the top's two pieces.
    assert!(close(a[0], 300.0) && close(a[5], 900.0), "{a:?}");
}
