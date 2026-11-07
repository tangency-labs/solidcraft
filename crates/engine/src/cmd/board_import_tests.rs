use std::f64::consts::PI;

use serde_json::{Value, json};
use solidcore_record::{Vec2, Vec3};
use solidcraft_io::{DrillArc, DrillHit, DrillSlot, Drills, Geom2};

use super::{Board, Finish, Origin, build_board, trace_steps};
use crate::Session;
use crate::schema::board_import::BoardImportResult;

fn v(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}

/// The outline of a `w` by `h` board with its lower-left corner at `at`.
fn rectangle(at: Vec2, w: f64, h: f64) -> Vec<Geom2> {
    let (a, b, c, d) = (at, at + v(w, 0.0), at + v(w, h), at + v(0.0, h));
    vec![Geom2::Line(a, b), Geom2::Line(b, c), Geom2::Line(c, d), Geom2::Line(d, a)]
}

fn hits(at: &[(f64, f64)], diameter: f64) -> Vec<DrillHit> {
    at.iter().map(|(x, y)| DrillHit { at: v(*x, *y), diameter, plated: Some(true), via: None }).collect()
}

fn build(s: &mut Session, outline: &[Geom2], drills: &Drills, thickness: f64, origin: Origin) -> BoardImportResult {
    let board = Board {
        outline,
        drills,
        thickness,
        name: "PCB",
        origin,
        outline_file: "pcb-Edge_Cuts.gm1",
        drill_files: &[],
        layers: &[],
        finish: Finish::Hasl,
    };
    build_board(s, &board, Vec::new()).unwrap_or_else(|e| panic!("{e}"))
}

/// The component's properties, by key.
fn property(s: &Session, component: u64, key: &str) -> Option<String> {
    s.doc.components.iter().find(|c| c.id == component).and_then(|c| c.properties.get(key).cloned())
}

/// Whether the sketch, the extrude and the body are all in `component`.
fn all_in(s: &Session, r: &BoardImportResult) -> bool {
    let in_it = |id: u64| s.doc.feature(id).is_some_and(|f| f.component == r.component);
    let st = s.model.state();
    let body = st.bodies.iter().find(|b| b.name == r.body).is_some_and(|b| s.doc.model_body_component(b) == r.component);
    in_it(r.sketch) && in_it(r.feature) && body
}

fn volume(s: &mut Session) -> f64 {
    let m = s.execute("inspect.measure", &json!({})).unwrap_or_else(|e| panic!("{e}"));
    m["total"]["volume_mm3"].as_f64().unwrap_or(f64::NAN)
}

fn near(got: f64, want: f64) {
    assert!((got - want).abs() < 1e-3 * want.abs().max(1.0), "volume {got}, expected {want}");
}

/// The four mounting holes of a 30 by 20 mm board: 3.2 mm, 3.5 mm in from each edge.
const CORNERS: [(f64, f64); 4] = [(3.5, 3.5), (26.5, 3.5), (3.5, 16.5), (26.5, 16.5)];

/// A 30 by 20 mm board with four 3.2 mm holes is one body of the board's volume less the holes',
/// in a new component named after the board that holds the sketch `Outline`, the extrude and the
/// body `Board`, with properties naming the files; building it adds no undo step of its own (the
/// command that runs it is the one step) and leaves the active component as it was.
#[test]
fn a_board_with_four_holes_has_the_volume_of_the_board_less_the_holes() {
    let mut s = Session::default();
    let undo = s.undo.len();
    let drills = Drills { hits: hits(&CORNERS, 3.2), ..Drills::default() };
    let r = build(&mut s, &rectangle(Vec2::ZERO, 30.0, 20.0), &drills, 1.6, Origin::File);
    assert_eq!((r.holes, r.slots, r.skipped, r.bodies.len()), (4, 0, 0, 1), "{r:?}");
    assert_eq!((r.body.as_str(), r.component_name.as_str()), ("Board", "PCB"));
    assert!(r.warnings.is_empty(), "{:?}", r.warnings);
    near(volume(&mut s), 30.0 * 20.0 * 1.6 - 4.0 * PI * 1.6 * 1.6 * 1.6);
    assert_eq!(s.doc.feature(r.sketch).map(|f| f.name.as_str()), Some("Outline"));
    assert!(all_in(&s, &r), "{r:?}");
    assert_eq!(property(&s, r.component, "pcb.outline").as_deref(), Some("pcb-Edge_Cuts.gm1"));
    assert_eq!(property(&s, r.component, "pcb.thickness").as_deref(), Some("1.6"));
    assert_eq!(property(&s, r.component, "pcb.drills"), None, "no drill files were given");
    assert!(s.active_sketch.is_none() && s.active_component == 0);
    assert_eq!(s.undo.len(), undo, "the inner steps push no undo steps");
    assert_eq!(s.model.state().bodies.iter().filter(|b| b.name == "Board").count(), 1);
    // It looks like a made board: laminate for the body, a look set on some of its faces (the
    // solder mask on top and bottom, copper in the plated holes).
    let chain = s.execute("appearance.chain", &json!({"target": {"body": "Board"}})).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(chain["active"], json!("material"), "{chain}");
    let faces = &s.doc.appearances.faces;
    assert_eq!(faces.iter().filter(|f| f.body == "Board").count(), 2 + 4, "top, bottom and four plated walls: {faces:?}");
    // On the evaluated board, the faces seen at those points are the ones coloured.
    s.refresh();
    let st = s.model.state();
    let board = st.bodies.iter().find(|b| b.name == "Board").unwrap_or_else(|| panic!("no Board"));
    let colours = s.doc.face_colors(board);
    for (z, want) in [(1.6, [0x1d, 0x6b, 0x40]), (0.0, [0x1d, 0x6b, 0x40])] {
        let face = solidcraft_doc::appearance::face_index_at(board, Vec3::new(15.0, 10.0, z)).unwrap_or_else(|| panic!("no face at z {z}"));
        let look = colours.iter().find(|(i, _)| *i == face).map(|(_, l)| l.color);
        assert_eq!(look, Some(want), "the mask at z {z}: {colours:?}");
    }
}

const RECTANGLE_GERBER: &str = "\
G04 A 30 by 20 mm board outline at (100, 50), hand-written*
%FSLAX46Y46*%
%MOMM*%
%ADD10C,0.100000*%
D10*
X100000000Y50000000D02*
X130000000Y50000000D01*
X130000000Y70000000D01*
X100000000Y70000000D01*
X100000000Y50000000D01*
M02*
";

const CORNER_DRILLS: &str = "M48\nMETRIC\nT1C3.200\n%\nG90\nG05\nT1\nX103.5Y53.5\nX126.5Y53.5\nX103.5Y66.5\nX126.5Y66.5\nM30\n";

/// The command end to end, from the files: the board is built, one undo step removes the
/// component and everything in it, and redo brings it back. A file that can't be read leaves no
/// component behind.
#[test]
fn inserting_a_board_from_its_files_is_one_undo_step() {
    let dir = std::env::temp_dir().join(format!("solidcraft-board-import-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let outline = dir.join("pcb-Edge_Cuts.gm1");
    let drills = dir.join("pcb-PTH.drl");
    std::fs::write(&outline, RECTANGLE_GERBER).unwrap_or_else(|e| panic!("{e}"));
    std::fs::write(&drills, CORNER_DRILLS).unwrap_or_else(|e| panic!("{e}"));
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length": 5, "width": 5, "height": 5, "corner": [0, -20, 0]})).unwrap_or_else(|e| panic!("{e}"));
    let (features, components, undo) = (s.doc.features.len(), s.doc.components.len(), s.undo.len());
    let p = json!({"outline": outline.to_string_lossy(), "drills": [drills.to_string_lossy()], "origin": "corner", "thickness": 1.2});
    let r = s.execute("file.insert_gerber", &p).unwrap_or_else(|e| panic!("{e}"));
    let typed = BoardImportResult {
        component: r["component"].as_u64().unwrap_or(0),
        component_name: r["component_name"].as_str().unwrap_or("").into(),
        sketch: r["sketch"].as_u64().unwrap_or(0),
        feature: r["feature"].as_u64().unwrap_or(0),
        body: r["body"].as_str().unwrap_or("").into(),
        bodies: Vec::new(),
        holes: 0,
        vias: 0,
        slots: 0,
        skipped: 0,
        thickness: 0.0,
        offset: [0.0, 0.0],
        layer_sketches: Vec::new(),
        prints: Vec::new(),
        warnings: Vec::new(),
        param_warnings: Vec::new(),
    };
    assert_eq!(
        (r["holes"].as_u64(), r["component_name"].as_str(), r["offset"].clone()),
        (Some(4), Some("pcb-Edge_Cuts"), json!([-100.0, -50.0])),
        "{r}"
    );
    assert!(all_in(&s, &typed), "{r}");
    assert_eq!(property(&s, typed.component, "pcb.drills").as_deref(), Some("pcb-PTH.drl"));
    assert_eq!(property(&s, typed.component, "pcb.thickness").as_deref(), Some("1.2"));
    near(volume(&mut s), 125.0 + (600.0 - 4.0 * PI * 1.6 * 1.6) * 1.2);
    assert_eq!(s.undo.len(), undo + 1, "one undo step");
    s.execute("edit.undo", &json!({})).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!((s.doc.features.len(), s.doc.components.len()), (features, components), "undo removes the whole board");
    near(volume(&mut s), 125.0);
    s.execute("edit.redo", &json!({})).unwrap_or_else(|e| panic!("{e}"));
    assert!(all_in(&s, &typed));
    // A drill file that isn't there: refused, and nothing is left of the board.
    s.execute("edit.undo", &json!({})).unwrap_or_else(|e| panic!("{e}"));
    let missing = json!({"outline": outline.to_string_lossy(), "drills": [dir.join("gone.drl").to_string_lossy()]});
    let e = s.execute("file.insert_gerber", &missing).map(|_| ()).unwrap_err().to_string();
    assert!(e.contains("gone.drl"), "{e}");
    assert_eq!((s.doc.features.len(), s.doc.components.len(), s.active_component), (features, components, 0));
    let _ = std::fs::remove_dir_all(&dir);
}

/// `thickness` is how far the board is extruded.
#[test]
fn the_thickness_is_the_boards() {
    for t in [0.8, 3.0] {
        let mut s = Session::default();
        let r = build(&mut s, &rectangle(Vec2::ZERO, 30.0, 20.0), &Drills::default(), t, Origin::File);
        assert_eq!(r.thickness, t);
        near(volume(&mut s), 600.0 * t);
    }
}

/// A routed slot is cut as its outline: two sides and a half circle round each end.
#[test]
fn a_slot_is_cut_through_the_board() {
    let mut s = Session::default();
    let drills = Drills { slots: vec![DrillSlot { a: v(10.0, 10.0), b: v(20.0, 12.0), diameter: 2.0, plated: None }], ..Drills::default() };
    let r = build(&mut s, &rectangle(Vec2::ZERO, 30.0, 20.0), &drills, 1.6, Origin::File);
    assert_eq!((r.holes, r.slots, r.skipped), (0, 1, 0), "{r:?}");
    let length = (10.0f64 * 10.0 + 2.0 * 2.0).sqrt();
    near(volume(&mut s), (600.0 - (length * 2.0 + PI)) * 1.6);
}

/// A slot routed along an arc is cut as its exact outline: two concentric arcs and a half circle
/// round each end. One that would cross the board's edge is skipped with a warning.
#[test]
fn a_curved_slot_is_cut_through_the_board() {
    let mut s = Session::default();
    let arc = |radius: f64| DrillArc { c: v(15.0, 10.0), a: v(15.0 + radius, 10.0), b: v(15.0 - radius, 10.0), diameter: 2.0, plated: None };
    let drills = Drills { arcs: vec![arc(5.0), arc(9.5)], ..Drills::default() };
    let r = build(&mut s, &rectangle(Vec2::ZERO, 30.0, 20.0), &drills, 1.6, Origin::File);
    assert_eq!((r.holes, r.slots, r.skipped), (0, 1, 1), "{r:?}");
    assert!(r.warnings.join("\n").contains("curved slot from (24.5, 10) to (5.5, 10)"), "{:?}", r.warnings);
    // Half an annulus from radius 4 to 6, and a whole 2 mm disc from the two end caps.
    near(volume(&mut s), (600.0 - (PI / 2.0 * (36.0 - 16.0) + PI)) * 1.6);
}

/// `origin: corner` moves the board's lower-left corner to the origin; `file` keeps the files'
/// coordinates. The holes move with the board.
#[test]
fn the_board_is_placed_at_the_origin_or_where_the_files_put_it() {
    for (origin, offset, lo) in [(Origin::Corner, [-100.0, -50.0], (0.0, 0.0)), (Origin::File, [0.0, 0.0], (100.0, 50.0))] {
        let mut s = Session::default();
        let drills = Drills { hits: hits(&[(105.0, 55.0)], 2.0), ..Drills::default() };
        let r = build(&mut s, &rectangle(v(100.0, 50.0), 30.0, 20.0), &drills, 1.6, origin);
        assert_eq!((r.offset, r.holes), (offset, 1), "{origin:?}");
        let st = s.model.state();
        let sk = &st.sketch(r.sketch).unwrap_or_else(|| panic!("no sketch")).sketch;
        // The sketch's own origin point is at (0, 0) either way, so look for the board's corners.
        let has = |x: f64, y: f64| sk.points.iter().any(|p| (p.pos.x - x).abs() < 1e-9 && (p.pos.y - y).abs() < 1e-9);
        assert!(has(lo.0, lo.1) && has(lo.0 + 30.0, lo.1 + 20.0), "{origin:?}: the board's corners");
        assert!(has(105.0 + offset[0], 55.0 + offset[1]), "{origin:?}: the hole moved with the board");
    }
}

/// A hole outside the board, or across its edge, is skipped with a warning naming it; the board
/// is built with the rest. Overlapping holes are cut as one.
#[test]
fn holes_off_the_board_are_skipped_with_a_warning_and_overlapping_ones_are_fine() {
    let mut s = Session::default();
    let mut drills = Drills { hits: hits(&[(40.0, 10.0), (0.5, 10.0), (10.0, 10.0), (12.0, 10.0)], 4.0), ..Drills::default() };
    drills.slots.push(DrillSlot { a: v(25.0, 25.0), b: v(25.0, 30.0), diameter: 1.0, plated: None });
    let r = build(&mut s, &rectangle(Vec2::ZERO, 30.0, 20.0), &drills, 1.6, Origin::File);
    assert_eq!((r.holes, r.slots, r.skipped), (2, 0, 3), "{r:?}");
    let all = r.warnings.join("\n");
    assert!(all.contains("4 mm hole at (40, 10)") && all.contains("(0.5, 10)") && all.contains("slot from (25, 25) to (25, 30)"), "{all}");
    // Two 4 mm holes 2 mm apart: their union is two discs less the lens they share.
    let (r2, d) = (2.0f64, 2.0f64);
    let lens = 2.0 * r2 * r2 * (d / (2.0 * r2)).acos() - (d / 2.0) * (4.0 * r2 * r2 - d * d).sqrt();
    near(volume(&mut s), (600.0 - (2.0 * PI * r2 * r2 - lens)) * 1.6);
}

/// A board with rounded corners (arcs) and a cut-out (a loop inside the outline): the cut-out is
/// a hole, a hole in it or in a corner's cut-away is skipped.
#[test]
fn arcs_and_cut_outs_shape_the_board() {
    let r = 2.0;
    let (w, h) = (30.0, 20.0);
    let mut outline = vec![
        Geom2::Line(v(r, 0.0), v(w - r, 0.0)),
        Geom2::Arc { c: v(w - r, r), a: v(w - r, 0.0), b: v(w, r) },
        Geom2::Line(v(w, r), v(w, h - r)),
        Geom2::Arc { c: v(w - r, h - r), a: v(w, h - r), b: v(w - r, h) },
        Geom2::Line(v(w - r, h), v(r, h)),
        Geom2::Arc { c: v(r, h - r), a: v(r, h), b: v(0.0, h - r) },
        Geom2::Line(v(0.0, h - r), v(0.0, r)),
        Geom2::Arc { c: v(r, r), a: v(0.0, r), b: v(r, 0.0) },
    ];
    outline.extend(rectangle(v(10.0, 5.0), 10.0, 10.0));
    let drills = Drills { hits: hits(&[(0.3, 0.3), (15.0, 10.0), (5.0, 10.0)], 0.6), ..Drills::default() };
    let mut s = Session::default();
    let b = build(&mut s, &outline, &drills, 1.6, Origin::File);
    assert_eq!((b.holes, b.skipped), (1, 2), "{b:?}");
    let area = w * h - 4.0 * (r * r - PI * r * r / 4.0) - 100.0 - PI * 0.3 * 0.3;
    near(volume(&mut s), area * 1.6);
}

/// An outline that doesn't close is refused, naming the gap; one with nothing on it too. The
/// design is not touched.
#[test]
fn an_open_outline_is_refused_naming_the_gap() {
    let mut s = Session::default();
    let mut outline = rectangle(Vec2::ZERO, 30.0, 20.0);
    outline[3] = Geom2::Line(v(0.0, 20.0), v(0.0, 0.5));
    let board = Board {
        outline: &outline,
        drills: &Drills::default(),
        thickness: 1.6,
        name: "PCB",
        origin: Origin::File,
        outline_file: "pcb.gm1",
        drill_files: &[],
        layers: &[],
        finish: Finish::Hasl,
    };
    let e = build_board(&mut s, &board, Vec::new()).map(|_| ()).unwrap_err().to_string();
    assert!(e.contains("not closed") && e.contains("gap of 0.5 mm") && e.contains("(0, 0.5)") && e.contains("(0, 0)"), "{e}");
    let empty = Board { outline: &[Geom2::Point(Vec2::ZERO)], ..board };
    let e = build_board(&mut s, &empty, Vec::new()).map(|_| ()).unwrap_err().to_string();
    assert!(e.contains("draws nothing"), "{e}");
    assert!(s.doc.features.is_empty() && s.doc.components.is_empty(), "nothing was added");
    // A 0.01 mm slip, as old files have, is closed with a line and a warning.
    let mut slipped = rectangle(Vec2::ZERO, 30.0, 20.0);
    slipped[3] = Geom2::Line(v(0.0, 20.0), v(0.0, 0.01));
    let r = build_board(&mut s, &Board { outline: &slipped, ..board }, Vec::new()).unwrap_or_else(|e| panic!("{e}"));
    assert!(r.warnings.iter().any(|w| w.contains("closed a gap of 0.01 mm")), "{:?}", r.warnings);
}

/// A drill file that doesn't give its number format, written with one more decimal place than
/// the usual guess, is read so its holes land on the board, saying so.
#[test]
fn a_drill_file_without_its_number_format_is_read_to_fit_the_board() {
    let mut s = Session::default();
    let drill = "M48
METRIC
T1C1.0
%
T1
X1100000Y600000
M30
";
    let r = s.execute("file.insert_gerber", &json!({"outline_text": RECTANGLE_GERBER, "drills_text": drill})).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(r["holes"], 1, "{r}");
    assert!(r["warnings"].to_string().contains("read with 4, not the usual 3"), "{r}");
}

/// Bad parameters are refused before any file is read, saying what is wrong.
#[test]
fn bad_parameters_are_refused() {
    let mut s = Session::default();
    let square = "%MOMM*%\nD10*\nX0Y0D02*\nX10000000Y0D01*\nM02*\n";
    let cases: [(Value, &str); 14] = [
        (json!({}), "give `outline`"),
        (json!({"outline": "a.gm1", "outline_text": square}), "not both"),
        (json!({"outline_text": square, "thickness": 0}), "`thickness`"),
        (json!({"outline_text": square, "thickness": -1}), "`thickness`"),
        (json!({"outline_text": square, "thickness": 25}), "`thickness`"),
        (json!({"outline_text": square, "thickness": "thick"}), "`thickness`"),
        (json!({"outline_text": square, "origin": "middle"}), "`origin`"),
        (json!({"outline_text": 5}), "`outline_text`"),
        (json!({"outline_text": square, "drills": [3]}), "`drills`"),
        (json!({"outline_text": square, "drills_text": vec![square; 17]}), "at most 16"),
        (json!({"outline_text": square, "layers": ["a.gtl"]}), "`layers` must be an object"),
        (json!({"outline_text": square, "layers": {"inner_copper": "a.g2"}}), "no layer `inner_copper`"),
        (json!({"outline_text": square, "layers": {"top_copper": "a.gtl"}, "layers_text": {"top_copper": square}}), "given twice"),
        (json!({"outline_text": square, "finish": "osp"}), "`finish`"),
    ];
    for (p, want) in cases {
        let e = s.execute("file.insert_gerber", &p).map(|_| ()).unwrap_err().to_string();
        assert!(e.contains(want), "{p}: {e}");
    }
    let e = s.execute("file.insert_gerber", &json!({"outline": "/nonexistent/board-Edge_Cuts.gm1"})).map(|_| ()).unwrap_err().to_string();
    assert!(e.contains("board-Edge_Cuts.gm1"), "{e}");
    assert!(s.doc.features.is_empty() && s.undo.is_empty(), "a refused insert changes nothing");
}

/// Copper, mask and silkscreen layers become hidden sketches in the board's component, and the top
/// gets its printed look as an image on the top face.
#[test]
fn layers_become_hidden_sketches_and_a_printed_look() {
    let mut s = Session::default();
    let head = "%FSLAX46Y46*%%MOMM*%";
    // A 2 mm square pad at (110, 60) and a 0.5 mm trace from it.
    let copper = format!("{head}%ADD10R,2X2*%%ADD11C,0.5*%D10*X110000000Y60000000D03*D11*X110000000Y60000000D02*X120000000Y60000000D01*M02*");
    let mask = format!("{head}%ADD10R,2.2X2.2*%D10*X110000000Y60000000D03*M02*");
    let silk = format!("{head}%ADD10C,0.2*%D10*X105000000Y55000000D02*X125000000Y55000000D01*M02*");
    let p = json!({"outline_text": RECTANGLE_GERBER, "layers_text": {"top_copper": copper, "top_mask": mask, "top_silk": silk}, "finish": "enig"});
    let r = s.execute("file.insert_gerber", &p).unwrap_or_else(|e| panic!("{e}"));
    let ids: Vec<u64> = r["layer_sketches"].as_array().map(|a| a.iter().filter_map(Value::as_u64).collect()).unwrap_or_default();
    assert_eq!(ids.len(), 3, "{r}");
    assert!(ids.iter().all(|id| s.visibility.hidden_sketches.contains(id)), "the layer sketches are hidden");
    assert_eq!(r["prints"], json!(["Top print"]), "{r}");
    let canvas = s.doc.canvases.iter().find(|c| c.name == "Top print").unwrap_or_else(|| panic!("no Top print"));
    // The image covers the 30 by 20 mm board at 20 pixels a mm.
    assert_eq!((canvas.pixels, canvas.width), ([600, 400], 30.0), "{canvas:?}");
    let component = s.doc.components.iter().find(|c| c.id == r["component"].as_u64().unwrap_or(0)).unwrap_or_else(|| panic!("no component"));
    assert_eq!(component.properties.get("pcb.top_mask").map(String::as_str), Some("top mask text"));
    assert_eq!(canvas.component, component.id);
}

/// Vias are kept in a hidden sketch and drawn, not cut: a hole the drill file marks as a via,
/// whatever its size, and an unmarked plated one of 0.5 mm or less. A marked component hole and
/// an unplated one are cut however small.
#[test]
fn vias_are_kept_in_a_sketch_not_cut() {
    let mut s = Session::default();
    let hole = |x: f64, diameter: f64, plated: Option<bool>, via: Option<bool>| DrillHit { at: v(x, 5.0), diameter, plated, via };
    let drills = Drills {
        hits: vec![
            hole(5.0, 0.4, Some(true), None),
            hole(10.0, 1.0, Some(true), Some(true)),
            hole(15.0, 0.4, Some(true), Some(false)),
            hole(20.0, 0.4, Some(false), None),
        ],
        ..Drills::default()
    };
    let r = build(&mut s, &rectangle(Vec2::ZERO, 30.0, 20.0), &drills, 1.6, Origin::File);
    assert_eq!((r.holes, r.vias), (2, 2), "{r:?}");
    assert_eq!(r.layer_sketches.len(), 1, "{r:?}");
    assert!(r.layer_sketches.iter().all(|id| s.visibility.hidden_sketches.contains(id)), "the vias' sketch is hidden");
    near(volume(&mut s), (600.0 - 2.0 * PI * 0.2 * 0.2) * 1.6);
}

/// An outline whose rounded corners are drawn as hundreds of tiny lines, as some writers do, is
/// read as the four lines and four arcs it traces, and builds the same board.
#[test]
fn an_outline_drawn_in_tiny_steps_is_read_as_its_arcs() {
    let (w, h, r) = (30.0, 20.0, 2.0);
    let corner = |c: Vec2, from: f64| -> Vec<Vec2> {
        (0..=200).map(|k| c + v((from + PI / 2.0 * k as f64 / 200.0).cos(), (from + PI / 2.0 * k as f64 / 200.0).sin()) * r).collect()
    };
    let mut pts = Vec::new();
    for (c, from) in [(v(w - r, r), -PI / 2.0), (v(w - r, h - r), 0.0), (v(r, h - r), PI / 2.0), (v(r, r), PI)] {
        pts.extend(corner(c, from));
    }
    let first = pts[0];
    pts.push(first);
    let outline: Vec<Geom2> = pts.windows(2).map(|s| Geom2::Line(s[0], s[1])).collect();
    let mut warnings = Vec::new();
    let traced = trace_steps(&outline, &mut warnings);
    let arcs = traced.iter().filter(|g| matches!(g, Geom2::Arc { .. })).count();
    assert_eq!((traced.len(), arcs), (8, 4), "{traced:?}");
    assert!(warnings.iter().any(|w| w.contains("800 short lines") && w.contains("the 4 arcs and lines")), "{warnings:?}");
    let mut s = Session::default();
    let _ = build(&mut s, &outline, &Drills::default(), 1.6, Origin::File);
    near(volume(&mut s), (w * h - (4.0 - PI) * r * r) * 1.6);
}
