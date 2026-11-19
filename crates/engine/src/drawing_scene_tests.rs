use std::sync::Arc;

use serde_json::{Value, json};
use solidcraft_doc::MAX_PLACEMENT_DEPTH;
use solidcraft_doc::drawing::DEFAULT_DPI;
use solidcraft_doc::drawing::view::{View, ViewKind, ViewPlace};
use solidcraft_doc::drawing_view::{Exploded, ViewDirection, ViewOverrides};
use solidcraft_geom::cell::Projection;
use solidcraft_geom::drawing_view::{NamedView, ProjectedSide};
use solidcraft_geom::paper::line::{LineGroup, LineKind};
use solidcraft_geom::paper::{NoPictures, ObjectKind};
use solidcraft_geom::pixel_grid::GridWindow;

use super::*;
use crate::schema::drawing_scene::{SheetPaperScene, SheetSceneResult};
use crate::schema::output_schema_for;
use crate::schema::tests::conforms;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap()
}

/// A block 100 x 50 x 20 mm along X, Y and Z from the origin, and a drawing of it on A3 with the
/// ISO border and no title block, so its scene is the border and the views: the front view at 1:2
/// centred at (150, 150), and the view above it, the top view in third angle, lined up with it at
/// y 220. The session, the sheet and the two views' ids.
pub(crate) fn front_and_top() -> (Session, u64, [u64; 2]) {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 100, "width": 50, "height": 20, "body_name": "Block"}));
    front_and_top_of(s)
}

/// The flat pattern view's plate (#13030): rule `T2` (2 mm thick, K-factor 0.5, inner radius 1 mm),
/// a 100 x 60 mm base flange named `Plate` on XY, a 20 mm flange bent up along its edge y = 0, and a
/// drawing of it, open.
pub(crate) fn plate() -> Session {
    let mut s = Session::default();
    let rule = json!({"name": "T2", "thickness": "2 mm", "k_factor": 0.5, "bend_radius": "1 mm", "gap": "0.5 mm", "active": true});
    run(&mut s, "sheet.manage_rules", rule);
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Base"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [100, 60]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sheet.flange", json!({"sketch": "Base", "body_name": "Plate"}));
    run(&mut s, "sheet.flange", json!({"edges": [[50, 0, 2]], "height": 20}));
    run(&mut s, "drawing.create", json!({}));
    s
}

/// [`front_and_top`]'s drawing of the design `s`.
fn front_and_top_of(mut s: Session) -> (Session, u64, [u64; 2]) {
    run(&mut s, "drawing.create", json!({}));
    let mut d = (**s.drawing("test").unwrap()).clone();
    Arc::make_mut(&mut d.sheets[0]).set_title_block(None);
    let mut id_source = solidcraft_doc::ids::IdSource::seeded(4890);
    let (sheet, source) = (d.sheets[0].id, d.sources[0].id);
    let mut add = |d: &mut Drawing, kind: ViewKind, place: ViewPlace, settings: ViewOverrides| {
        let id = d.allocate_id(&mut id_source).unwrap();
        d.add_views([Arc::new(View { id, sheet, kind, place, settings, reference_shown_percent: 0.0, label: None })]);
        id
    };
    let half = ViewOverrides { scale: Some("1:2".parse().unwrap()), ..ViewOverrides::default() };
    let front = add(&mut d, ViewKind::Base { source, view: ViewDirection::Named(NamedView::Front) }, ViewPlace::At([150.0, 150.0]), half);
    let top = add(&mut d, ViewKind::Projected { parent: front, side: ProjectedSide::Above }, ViewPlace::Aligned(220.0), ViewOverrides::default());
    d.validate().unwrap();
    s.doc_mut().put_drawing(Arc::new(d));
    (s, sheet, [front, top])
}

/// Component `L{level}`, placed inside `L{level - 1}` (the design itself at level 0).
pub(crate) fn nest(s: &mut Session, level: u64) {
    let (component, parent) = (1000 + level, if level == 0 { 0 } else { 1000 + level - 1 });
    let doc = s.doc_mut();
    doc.components.push(solidcraft_doc::Component { id: component, name: format!("L{level}"), parent, ..Default::default() });
    let name = format!("L{level}");
    // From JSON, so a field another change adds to occurrences takes its default here.
    let occurrence = json!({"id": 5000 + level, "component": component, "parent": parent, "name": name});
    doc.occurrences.push(serde_json::from_value(occurrence).unwrap());
}

/// Part `Part` with a 10 mm cube `Cube` from the origin, placed twice: the second copy 30 mm along
/// X. The session and the two occurrences.
pub(crate) fn two_cubes() -> (Session, [u64; 2]) {
    let mut s = Session::default();
    let created = run(&mut s, "component.create", json!({"name": "Part"}));
    let first = created["occurrence"].as_u64().unwrap();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Cube", "operation": "new"}));
    let second = run(&mut s, "occurrence.copy", json!({"occurrence": first, "translate": [30, 0, 0]}))["occurrence"].as_u64().unwrap();
    (s, [first, second])
}

/// A hinge: the design's own plate, 40 x 20 x 2 mm from the origin, and a leaf the same size in
/// component Leaf beyond it along Y, on revolute joint Hinge about their shared edge (along X at y 20,
/// z 0), at 0 as the design places it; and a drawing, open, with a top view of both at 1:1 on
/// (150, 150). The session, the view and the joint's id.
pub(crate) fn hinge_pair() -> (Session, u64, u64) {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 20, "height": 2, "body_name": "Plate"}));
    run(&mut s, "component.create", json!({"name": "Leaf", "grounded": false}));
    run(&mut s, "solid.box", json!({"length": 40, "width": 20, "height": 2, "corner": [0, 20, 0], "body_name": "Leaf", "operation": "new"}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let at = json!({"occurrence": 0, "point": [0, 20, 0], "z": [1, 0, 0]});
    run(&mut s, "joint.as_built", json!({"type": "revolute", "name": "Hinge", "a": 0, "b": "Leaf", "at": at}));
    let hinge = s.doc.assembly.joints.iter().find(|j| j.name == "Hinge").map(|j| j.id).unwrap();
    run(&mut s, "drawing.create", json!({}));
    let sheet = ids::encode(s.drawing("test").unwrap().sheets[0].id);
    let p = json!({"sheet": sheet, "view": "top", "scale": "1:1", "position_mm": [150, 150]});
    let view = ids::decode(run(&mut s, "drawing.view.base", p)["view"].as_str().unwrap()).unwrap();
    (s, view, hinge)
}

/// A second component, so Exploded View can be made (it needs two): a 4 x 4 x 2 mm pin in component
/// Pin on the hinge's plate, inside the plate's outline from above, which no explode here moves.
pub(crate) fn add_a_pin(s: &mut Session) {
    run(s, "component.create", json!({"name": "Pin"}));
    run(s, "solid.box", json!({"length": 4, "width": 4, "height": 2, "corner": [18, 8, 2], "body_name": "Pin", "operation": "new"}));
    run(s, "component.activate", json!({"component": "root"}));
}

/// A block 100 x 50 x 20 mm along X, Y and Z from the origin, its length parameter `L`, with
/// configuration rows Default (as it is, the active row) and Long (`L` = 120 mm): the session and
/// Long's row id.
pub(crate) fn configured_block() -> (Session, u64) {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "L", "expression": "100 mm"}));
    run(&mut s, "solid.box", json!({"length": "L", "width": 50, "height": 20, "body_name": "Block"}));
    run(&mut s, "config.configure", json!({"params": ["L"]}));
    let table = run(&mut s, "config.row", json!({"name": "Long", "values": {"L": "120 mm"}}));
    let long = table["rows"].as_array().and_then(|rows| rows.iter().find(|row| row["name"] == "Long")).and_then(|row| row["id"].as_u64());
    (s, long.unwrap())
}

/// [`front_and_top`]'s drawing of [`configured_block`], so each view is `L` / 2 wide on paper: the
/// session, the sheet, the two views and Long's row id.
pub(crate) fn configured_front_and_top() -> (Session, u64, [u64; 2], u64) {
    let (s, long) = configured_block();
    let (s, sheet, views) = front_and_top_of(s);
    (s, sheet, views, long)
}

/// Sheet `sheet` of the drawing open, built at once.
fn built(s: &Session, sheet: u64) -> SheetScene {
    let d = s.drawing("test").unwrap().clone();
    build(s, &d, sheet).unwrap()
}

/// The provider's scene of `sheet` once nothing is waiting, asked for as the window does: what it
/// shows, then a short piece of work, until there is none and no text waits for its font.
fn settled(s: &mut Session, sheet: u64) -> SheetView {
    let started = std::time::Instant::now();
    extract(s, sheet).unwrap();
    loop {
        let more = step(s);
        // Read after the step: its own read of a dimension can leave the dimension's text waiting.
        let answer = extract(s, sheet).unwrap();
        if !more && !answer.waiting_for_fonts {
            return answer;
        }
        assert!(started.elapsed().as_secs() < 120, "the sheet never settled");
        std::thread::yield_now();
    }
}

/// The scene as its JSON text: what byte for byte compares.
fn bytes(scene: &Scene) -> String {
    serde_json::to_string(scene).unwrap()
}

/// `record`'s title block text with no cells, in the built-in font: its labels only.
fn labels(record: &Sheet) -> Set {
    block_text(record, &Lettering { units: String::new(), language: String::new(), font: String::new() }, &[], true).unwrap().0
}

/// `group` without its text: a title block's lines and symbol, as its layout draws them.
fn without_text(group: &Group) -> Group {
    Group { items: group.items.iter().filter(|item| !matches!(item, Item::Text(_))).cloned().collect(), ..group.clone() }
}

fn border(s: &Session, sheet: u64) -> Vec<Group> {
    let d = s.drawing("test").unwrap();
    let record = d.sheets.iter().find(|x| x.id == sheet).unwrap();
    record.border.as_ref().unwrap().layout(&record.sheet).unwrap().paper(sheet)
}

/// The box round the ends of `group`'s stroke pieces, and how long they are in all.
fn reach(group: &Group) -> (Vec2, Vec2, f64) {
    let (mut lo, mut hi, mut length) = (Vec2::new(f64::INFINITY, f64::INFINITY), Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY), 0.0);
    for item in &group.items {
        let Item::Stroke { path, .. } = item else { continue };
        for seg in path {
            for p in [seg.start(), seg.end()] {
                lo = Vec2::new(lo.x.min(p.x), lo.y.min(p.y));
                hi = Vec2::new(hi.x.max(p.x), hi.y.max(p.y));
            }
            length += seg.length();
        }
    }
    (lo, hi, length)
}

/// Whether `a` and `b` are the same strokes in the same order, their pieces' ends within `tol`.
fn same_within(a: &Group, b: &Group, tol: f64) -> bool {
    let near = |s: &Seg2, t: &Seg2| s.start().dist(t.start()) <= tol && s.end().dist(t.end()) <= tol;
    a.items.len() == b.items.len()
        && a.items.iter().zip(&b.items).all(|pair| match pair {
            (Item::Stroke { path: p, closed: c, width_mm: w, .. }, Item::Stroke { path: q, closed: d, width_mm: v, .. }) => {
                c == d && w == v && p.len() == q.len() && p.iter().zip(q).all(|(s, t)| near(s, t))
            }
            _ => false,
        })
}

/// A real sheet editor under inch display units; command coordinates still mean paper mm.
fn stroke_editor() -> (Session, u64, u64) {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":40,"width":30,"height":5,"body_name":"Source plate"}));
    run(&mut s, "document.units", json!({"units":"in"}));
    run(&mut s, "parameters.add", json!({"name":"Inset","expression":"1 in"}));
    let drawing = run(&mut s, "drawing.create", json!({"name":"Strokes","units":"in"}));
    let made = run(&mut s, "drawing.sketch.create", json!({"owner":drawing["sheets"][0]["id"]}));
    let owner = ids::decode(made["drawing"].as_str().unwrap()).unwrap();
    let sketch = ids::decode(made["sketch"].as_str().unwrap()).unwrap();
    (s, owner, sketch)
}

fn stroke_path(item: &Item) -> (&[Seg2], bool, f64) {
    let Item::Stroke { path, closed, width_mm, pattern: None } = item else { panic!("not a stroke: {item:?}") };
    (path, *closed, *width_mm)
}

#[test]
fn sheet_sketch_strokes_keep_paper_coordinates_and_read_only_source() {
    let (mut s, owner, id) = stroke_editor();
    run(&mut s, "sketch.rectangle.two_point", json!({"p0":[15,10],"p1":[55,30]}));
    run(&mut s, "sketch.line", json!({"points":[["Inset + 5","40"],["Inset + 20","40"]],"infer":false}));
    let before = s.doc.clone();
    let model = s.model.state();
    let history = (s.undo.len(), s.redo.len(), s.evaluations);
    let solved = s.doc.solve_sheet_sketch(owner, id).unwrap();
    let group = sketch::paper_group(&solved, 0.35).unwrap();
    assert_eq!((group.object, group.kind, group.items.len()), (id, ObjectKind::Sketch, 5));
    assert_eq!(group.kind.layer(), "SKETCH");
    let mut rectangle = group.clone();
    rectangle.items.truncate(4);
    let (lo, hi, perimeter) = reach(&rectangle);
    assert!(lo.dist(Vec2::new(15.0, 10.0)) < 1e-9 && hi.dist(Vec2::new(55.0, 30.0)) < 1e-9);
    assert!((perimeter - 120.0).abs() < 1e-9);
    let (line, closed, width) = stroke_path(&group.items[4]);
    assert!(!closed && width == 0.35 && line.len() == 1);
    assert!(line[0].start().dist(Vec2::new(30.4, 40.0)) < 1e-9);
    assert!(line[0].end().dist(Vec2::new(45.4, 40.0)) < 1e-9);
    assert!(Arc::ptr_eq(&before, &s.doc) && Arc::ptr_eq(&model, &s.model.state()));
    assert_eq!((s.undo.len(), s.redo.len(), s.evaluations), history);
    assert_eq!(s.doc.drawing(owner).unwrap().sheet_sketch(id).unwrap().sketch, solved.sketch);

    // Reopening and solving through the same read-only boundary gives the same writer input.
    let reopened = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
    let again = sketch::paper_group(&reopened.solve_sheet_sketch(owner, id).unwrap(), 0.35).unwrap();
    assert_eq!(again, group);
    let scene = Scene::new(Vec2::new(420.0, 297.0), vec![group]);
    let svg = || solidcraft_io::paper::paper_svg(&scene.prepare().unwrap(), &NoPictures, DEFAULT_DPI).unwrap().0;
    assert_eq!(svg(), svg());
    assert!(String::from_utf8(svg()).unwrap().contains("data-kind=\"sketch\""));
}

#[test]
fn sheet_sketch_strokes_preserve_analytic_curves_and_clockwise_travel() {
    let (mut s, owner, id) = stroke_editor();
    for (cmd, params) in [
        ("sketch.line", json!({"points":[[5,5],[15,5]],"infer":false})),
        ("sketch.circle.center", json!({"center":[30,40],"radius":5})),
        ("sketch.arc.center_point", json!({"center":[30,40],"start":[35,40],"sweep":-90})),
        ("sketch.ellipse", json!({"center":[80,50],"major":[100,50],"minor_radius":10})),
        ("sketch.conic", json!({"start":[40,80],"end":[90,80],"apex":[65,105],"rho":0.4,"connect":false})),
    ] {
        run(&mut s, cmd, params);
    }
    let solved = s.doc.solve_sheet_sketch(owner, id).unwrap();
    let group = sketch::paper_group(&solved, 0.5).unwrap();
    assert_eq!(group.items.len(), 5);
    assert!(matches!(stroke_path(&group.items[0]).0, [Seg2::Line { .. }]));
    let (circle, closed, _) = stroke_path(&group.items[1]);
    assert!(closed && circle.len() == 1);
    for seg in circle {
        let Seg2::Arc { center, radius, sweep, .. } = seg else { panic!("sampled circle: {seg:?}") };
        assert_eq!((*center, *radius), (Vec2::new(30.0, 40.0), 5.0));
        assert!((sweep - std::f64::consts::TAU).abs() < 1e-12);
    }
    let (arc, closed, _) = stroke_path(&group.items[2]);
    let [Seg2::Arc { radius, sweep, .. }] = arc else { panic!("sampled arc: {arc:?}") };
    assert!(!closed && *radius == 5.0 && (*sweep + std::f64::consts::FRAC_PI_2).abs() < 1e-12);
    assert!(arc[0].start().dist(Vec2::new(35.0, 40.0)) < 1e-9 && arc[0].end().dist(Vec2::new(30.0, 35.0)) < 1e-9);
    let (ellipse, closed, _) = stroke_path(&group.items[3]);
    assert!(closed && ellipse.len() == 4);
    for seg in ellipse {
        assert!(matches!(seg, Seg2::Conic { w, .. } if (*w - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-15));
        for t in [0.0, 0.2, 0.5, 0.8, 1.0] {
            let p = seg.point_at(t) - Vec2::new(80.0, 50.0);
            assert!((p.x * p.x / 400.0 + p.y * p.y / 100.0 - 1.0).abs() < 1e-12, "{p:?}");
        }
    }
    let (conic, closed, _) = stroke_path(&group.items[4]);
    let [Seg2::Conic { a, apex, b, w }] = conic else { panic!("sampled conic: {conic:?}") };
    assert!(!closed && (*w - 2.0 / 3.0).abs() < 1e-12);
    assert_eq!((*a, *apex, *b), (Vec2::new(40.0, 80.0), Vec2::new(65.0, 105.0), Vec2::new(90.0, 80.0)));
    assert!(conic[0].point_at(0.5).dist(Vec2::new(65.0, 90.0)) < 1e-9);
}

#[test]
fn sheet_sketch_strokes_keep_fit_periodic_and_quintic_splines_exact() {
    let (mut s, owner, id) = stroke_editor();
    run(&mut s, "sketch.spline.fit_point", json!({"points":[[10,10],[12,12],[14,10]],"connect":false}));
    run(&mut s, "sketch.spline.fit_point", json!({"points":[[80,30],[100,50],[120,30],[100,10]],"periodic":true,"connect":false}));
    let controls = [[140, 10], [150, 30], [160, 35], [170, 35], [180, 30], [190, 10]];
    run(&mut s, "sketch.spline.control_point_5", json!({"points":controls,"connect":false}));
    let group = sketch::paper_group(&s.doc.solve_sheet_sketch(owner, id).unwrap(), 0.5).unwrap();
    assert_eq!(group.items.len(), 3);
    let (fit, closed, _) = stroke_path(&group.items[0]);
    assert!(!closed && fit.len() == 2 && fit.iter().all(|seg| matches!(seg, Seg2::Cubic { .. })));
    let Seg2::Cubic { p0, p1, p2, p3 } = fit[0] else { panic!("lost cubic") };
    for (actual, expected) in [p0, p1, p2, p3].into_iter().zip([[10.0, 10.0], [10.0 + 2.0 / 3.0, 11.0], [10.0 + 4.0 / 3.0, 12.0], [12.0, 12.0]]) {
        assert!(actual.dist(Vec2::from(expected)) < 1e-9, "{actual:?} {expected:?}");
    }
    assert!(fit[0].point_at(0.5).dist(Vec2::new(11.0, 11.375)) < 1e-9);
    for (seg, a, b) in fit.iter().zip([[10.0, 10.0], [12.0, 12.0]]).zip([[12.0, 12.0], [14.0, 10.0]]).map(|((s, a), b)| (s, a, b)) {
        assert!(seg.start().dist(Vec2::from(a)) < 1e-9 && seg.end().dist(Vec2::from(b)) < 1e-9);
    }
    let (periodic, closed, _) = stroke_path(&group.items[1]);
    assert!(closed && periodic.len() == 4 && periodic.iter().all(|seg| matches!(seg, Seg2::Cubic { .. })));
    for i in 0..periodic.len() {
        let next = &periodic[(i + 1) % periodic.len()];
        assert!(periodic[i].end().dist(next.start()) < 1e-9);
        let end = periodic[i].derivatives(1.0, 2).unwrap();
        let start = next.derivatives(0.0, 2).unwrap();
        assert!(end[0].dist(start[0]) < 1e-8 && end[1].dist(start[1]) < 1e-8);
    }
    let (quintic, closed, _) = stroke_path(&group.items[2]);
    let [Seg2::Bezier { n: 5, p }] = quintic else { panic!("lost quintic: {quintic:?}") };
    assert!(!closed);
    for (actual, expected) in p.iter().zip(controls) {
        assert!(actual.dist(Vec2::new(expected[0] as f64, expected[1] as f64)) < 1e-9);
    }
}

#[test]
fn sheet_sketch_strokes_omit_construction_and_refuse_centerlines_by_name() {
    let (mut s, owner, id) = stroke_editor();
    run(&mut s, "sketch.line", json!({"points":[[10,10],[30,10]],"infer":false}));
    let mut solved = s.doc.solve_sheet_sketch(owner, id).unwrap();
    let curve = solved.sketch.curves[0].id.clone();
    let original = sketch::paper_group(&solved, 0.5).unwrap();
    solved.sketch.view.hide_grid = true;
    solved.sketch.view.no_snap = true;
    assert_eq!(sketch::paper_group(&solved, 0.5).unwrap(), original, "editor visibility is not print state");
    solved.sketch.curves[0].construction = true;
    solved.sketch.curves[0].infinite = true;
    assert!(sketch::paper_group(&solved, 0.5).unwrap().items.is_empty());
    for construction in [false, true] {
        solved.sketch.curves[0].infinite = false;
        solved.sketch.curves[0].construction = construction;
        solved.sketch.curves[0].centerline = true;
        let why = sketch::paper_group(&solved, 0.5).unwrap_err().to_string();
        assert!(
            why.contains(&ids::encode(owner)) && why.contains(&ids::encode(solved.sheet)) && why.contains(&ids::encode(id)) && why.contains(&curve),
            "{why}"
        );
        assert!(why.contains("centerline"), "{why}");
    }
}

#[test]
fn sheet_sketch_strokes_do_not_close_a_short_open_curve_by_tolerance() {
    let (mut s, owner, id) = stroke_editor();
    run(&mut s, "sketch.line", json!({"points":[[10,10],[30,10]],"infer":false}));
    let mut solved = s.doc.solve_sheet_sketch(owner, id).unwrap();
    solved.sketch.points[2].pos = solved.sketch.points[1].pos + Vec2::new(solidcraft_geom::EPS / 2.0, 0.0);
    let group = sketch::paper_group(&solved, 0.5).unwrap();
    let (path, closed, _) = stroke_path(&group.items[0]);
    assert!(!closed && path[0].start() != path[0].end());
    assert!(path[0].start().dist(path[0].end()) < solidcraft_geom::EPS);
}

#[test]
fn sheet_sketch_strokes_refuse_hostile_geometry_without_a_polyline_fallback() {
    use solidcraft_sketch::{CurveKind, SolveStatus};
    let (mut s, owner, id) = stroke_editor();
    run(&mut s, "sketch.line", json!({"points":[[10,10],[30,10]],"infer":false}));
    let original = s.doc.solve_sheet_sketch(owner, id).unwrap();
    for width in [0.0, -1.0, 10.0001, f64::NAN, f64::INFINITY] {
        assert!(sketch::paper_group(&original, width).is_err(), "width {width}");
    }
    let mut cases = Vec::new();
    let mut add = |change: fn(&mut solidcraft_doc::drawing::SolvedSheetSketch)| {
        let mut bad = original.clone();
        change(&mut bad);
        cases.push(bad);
    };
    add(|s| s.id = 0);
    add(|s| s.report.status = SolveStatus::Failed);
    add(|s| s.sketch.points[1].pos.x = f64::NAN);
    add(|s| s.sketch.points[1].pos.x = 100_001.0);
    add(|s| s.sketch.curves[0].kind = CurveKind::Line { a: 1, b: usize::MAX });
    add(|s| {
        s.sketch.curves[0].kind = CurveKind::Spline { pts: vec![1, 2], control: true, degree: 10, periodic: false, nurbs: None, handles: Vec::new() }
    });
    add(|s| s.sketch.curves[0].kind = CurveKind::Ellipse { c: 1, m: 1, r: 5.0 });
    add(|s| s.sketch.curves[0].kind = CurveKind::Conic { a: 1, b: 2, apex: 0, rho: 1.0 - 1e-10 });
    add(|s| s.sketch.curves[0].kind = CurveKind::Arc { c: 0, a: 1, b: 2 });
    add(|s| s.sketch.curves[0].infinite = true);
    add(|s| s.sketch.view.three_d = true);
    add(|s| s.sketch.curves[0].link = Some("unresolved".into()));
    add(|s| {
        s.sketch.equations.push(solidcraft_sketch::EquationCurve {
            curve: s.sketch.curves[0].id.clone(),
            x: None,
            y: "x".into(),
            start: "0".into(),
            end: "1".into(),
            tolerance: 0.01,
            variable_unit: Default::default(),
        })
    });
    for bad in cases {
        let why = sketch::paper_group(&bad, 0.5).unwrap_err().to_string();
        assert!(why.contains("sheet sketch"), "{why}");
    }
    assert_eq!(sketch::paper_group(&original, 0.5).unwrap().items.len(), 1, "no partial output or mutation after errors");
}

#[test]
fn sheet_sketch_strokes_respect_the_exact_paper_record_limit() {
    use solidcraft_sketch::{CurveKind, Sketch};
    let (s, owner, id) = stroke_editor();
    let mut solved = s.doc.solve_sheet_sketch(owner, id).unwrap();
    let mut geometry = Sketch::new();
    let centre = geometry.add_point(Vec2::new(100.0, 100.0), None).unwrap();
    let major = geometry.add_point(Vec2::new(110.0, 100.0), None).unwrap();
    // 19,998 ellipse strokes at 5 records each, plus the group itself: 99,991.
    let ellipse = geometry.add_curve(CurveKind::Ellipse { c: centre, m: major, r: 5.0 }, None).unwrap();
    let template = geometry.curves[ellipse].clone();
    geometry.curves = (0..19_998).map(|i| solidcraft_sketch::Curve { id: format!("ellipse{i}"), ..template.clone() }).collect();
    let pts = (0..9).map(|i| geometry.add_point(Vec2::new(20.0 + i as f64, 20.0 + (i % 2) as f64), None).unwrap()).collect();
    geometry.add_curve(CurveKind::Spline { pts, control: false, degree: 3, periodic: false, nurbs: None, handles: Vec::new() }, None).unwrap();
    solved.sketch = geometry;
    let full = sketch::paper_group(&solved, 0.5).unwrap();
    assert_eq!(full.records(), MAX_RECORDS, "eight cubic pieces plus their stroke consume the remaining nine records");
    solved.sketch.add_curve(CurveKind::Line { a: centre, b: major }, None).unwrap();
    let why = sketch::paper_group(&solved, 0.5).unwrap_err().to_string();
    assert!(why.contains(&MAX_RECORDS.to_string()), "{why}");
    assert_eq!(full.records(), MAX_RECORDS);
}

/// The scene is the sheet's three border groups as its layout draws them, byte for byte, then the
/// two views in id order: the front view round its 50 x 10 mm outline centred at (150, 150), 120 mm
/// of line in all, the top view round 50 x 25 mm centred at (150, 220), 150 mm. The command answers
/// the same scene, matching its schema, and the SVG writer takes it with the border's three groups
/// before the views'.
#[test]
fn a_sheet_is_its_border_then_its_views_by_id() {
    let (mut s, sheet, [front, top]) = front_and_top();
    let SheetScene { scene, views, .. } = built(&s, sheet);
    let border = border(&s, sheet);
    assert_eq!(border.len(), 3);
    assert_eq!(serde_json::to_string(&scene.groups[..3]).unwrap(), serde_json::to_string(&border).unwrap());
    let order = if front < top { [front, top] } else { [top, front] };
    let tail: Vec<(u64, ObjectKind)> = scene.groups[3..].iter().map(|g| (g.object, g.kind)).collect();
    assert_eq!(tail, order.map(|id| (id, ObjectKind::View)));
    for (id, lo, hi, length) in [(front, [125.0, 145.0], [175.0, 155.0], 120.0), (top, [125.0, 207.5], [175.0, 232.5], 150.0)] {
        let (min, max, drawn) = reach(scene.groups.iter().find(|g| g.object == id).unwrap());
        assert!(min.dist(Vec2::from(lo)) < 1e-6 && max.dist(Vec2::from(hi)) < 1e-6 && (drawn - length).abs() < 1e-6, "{id}: {min:?} {max:?} {drawn}");
    }
    let statuses: Vec<(u64, Status)> = views.iter().map(|v| (v.view, v.status.clone())).collect();
    assert_eq!(statuses, order.map(|id| (id, Status::Drawn)));

    let reply = run(&mut s, CMD, json!({"sheet": "Sheet 1"}));
    let schema = output_schema_for::<SheetSceneResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &reply), "{reply}");
    assert_eq!(reply["scene"], serde_json::to_value(&scene).unwrap());
    let replied: Vec<(Value, Value)> = reply["views"].as_array().unwrap().iter().map(|v| (v["view"].clone(), v["status"].clone())).collect();
    assert_eq!(replied, order.map(|id| (json!(ids::encode(id)), json!("drawn"))));

    let (svg, _) = solidcraft_io::paper::paper_svg(&scene.prepare().unwrap(), &NoPictures, DEFAULT_DPI).unwrap();
    let svg = String::from_utf8(svg).unwrap();
    let borders: Vec<usize> = svg.match_indices("data-kind=\"drawing_border\"").map(|(at, _)| at).collect();
    let first_view = svg.find("data-kind=\"view\"").unwrap();
    assert!(borders.len() == 3 && borders.iter().all(|at| *at < first_view), "{svg}");
}

/// Each view's group is the snapshot's for the same view, scale and centre: the same strokes in the
/// same order, their ends within rounding. The scene works a view's lines out centred on it and
/// then moves them; the snapshot maps them to the centre in one step.
#[test]
fn each_view_draws_as_its_snapshot_within_rounding() {
    let (mut s, sheet, [front, top]) = front_and_top();
    let scene = built(&s, sheet).scene;
    // The snapshot works on the model, so with no drawing open.
    run(&mut s, "drawing.close", json!({}));
    for (id, named, at) in [(front, "front", [150.0, 150.0]), (top, "top", [150.0, 220.0])] {
        let v = run(&mut s, "drawing.view.snapshot", json!({"view": named, "scale": "1:2", "position_mm": at, "scene": true}));
        let snapped: Scene = serde_json::from_value(v["scene"].clone()).unwrap();
        let ours = scene.groups.iter().find(|g| g.object == id).unwrap();
        assert!(same_within(ours, &snapped.groups[0], 1e-9), "{id}: {ours:?} {:?}", snapped.groups[0]);
    }
}

/// A view of many lines is drawn whole, as its snapshot is: the block drilled with eight holes and
/// seen from above is its outline and the eight rims, some 30 records.
#[test]
fn a_view_of_many_lines_draws_as_its_snapshot() {
    let (mut s, sheet, [_, top]) = front_and_top();
    let drawing = ids::encode(s.open_drawing.unwrap());
    run(&mut s, "drawing.close", json!({}));
    for i in 0..8 {
        let base = [12.5 + 25.0 * (i % 4) as f64, 12.5 + 25.0 * (i / 4) as f64, -1.0];
        run(&mut s, "solid.cylinder", json!({"diameter": 8, "height": 22, "base": base, "operation": "cut", "targets": ["Block"]}));
    }
    let v = run(&mut s, "drawing.view.snapshot", json!({"view": "top", "scale": "1:2", "position_mm": [150.0, 220.0], "scene": true}));
    let snapped: Scene = serde_json::from_value(v["scene"].clone()).unwrap();
    run(&mut s, "drawing.open", json!({"drawing": drawing}));
    let scene = built(&s, sheet);
    let state = scene.views.iter().find(|v| v.view == top).unwrap();
    assert_eq!(state.status, Status::Drawn, "{state:?}");
    let ours = scene.scene.groups.iter().find(|g| g.object == top).unwrap();
    assert!(ours.records() > 20 && same_within(ours, &snapped.groups[0], 1e-9), "{ours:?} {:?}", snapped.groups[0]);
}

/// A sheet's title block is drawn last, over its views, as its layout draws it, the sheet its
/// object, and the command's reply still matches its schema. Taking it away is a new scene from the
/// provider, and projects no view again.
#[test]
fn a_sheets_title_block_is_drawn_after_its_views() {
    let (mut s, sheet, [front, top]) = front_and_top();
    let block = solidcraft_geom::title_block::default_block();
    run(&mut s, "drawing.sheet.set", json!({"sheet": ids::encode(sheet), "title_block": serde_json::to_value(&block).unwrap()}));
    let scene = built(&s, sheet).scene;
    let order = if front < top { [front, top] } else { [top, front] };
    let mut expected: Vec<(u64, ObjectKind)> = border(&s, sheet).iter().map(|g| (g.object, g.kind)).collect();
    expected.extend(order.map(|id| (id, ObjectKind::View)));
    expected.push((sheet, ObjectKind::TitleBlock));
    assert_eq!(scene.groups.iter().map(|g| (g.object, g.kind)).collect::<Vec<_>>(), expected);
    let d = s.drawing("test").unwrap();
    let record = d.sheets.iter().find(|x| x.id == sheet).unwrap();
    let drawn = block.layout(&record.border.as_ref().unwrap().layout(&record.sheet).unwrap()).unwrap().paper(sheet, Projection::Third);
    assert_eq!(serde_json::to_string(&without_text(scene.groups.last().unwrap())).unwrap(), serde_json::to_string(&drawn).unwrap());
    let reply = run(&mut s, CMD, json!({"sheet": ids::encode(sheet)}));
    let schema = output_schema_for::<SheetSceneResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &reply), "{reply}");
    let before = settled(&mut s, sheet);
    assert_eq!(bytes(&before.scene), bytes(&scene));
    let projections = s.drawing_scenes.projections;
    run(&mut s, "drawing.sheet.set", json!({"sheet": ids::encode(sheet), "title_block": null}));
    let after = extract(&mut s, sheet).unwrap();
    assert!(after.generation > before.generation && after.scene.groups.iter().all(|g| g.kind != ObjectKind::TitleBlock));
    assert_eq!((after.stale.len(), s.drawing_scenes.projections), (0, projections));
}

/// The title block's symbol follows the drawing's projection: the first scene after a change of
/// projection already has the block drawn in it, while the projected view still waits to be drawn
/// again, and once it is, the scene is the one built at once.
#[test]
fn the_title_blocks_symbol_follows_the_drawings_projection() {
    let (mut s, sheet, _) = front_and_top();
    let block = solidcraft_geom::title_block::default_block();
    run(&mut s, "drawing.sheet.set", json!({"sheet": ids::encode(sheet), "title_block": serde_json::to_value(&block).unwrap()}));
    let block_of = |s: &Session, projection: Projection| {
        let d = s.drawing("test").unwrap();
        let record = d.sheets.iter().find(|x| x.id == sheet).unwrap();
        let border = record.border.as_ref().unwrap().layout(&record.sheet).unwrap();
        serde_json::to_string(&record.title_block.as_ref().unwrap().layout(&border).unwrap().paper(sheet, projection)).unwrap()
    };
    let third = settled(&mut s, sheet);
    assert_eq!(serde_json::to_string(&without_text(third.scene.groups.last().unwrap())).unwrap(), block_of(&s, Projection::Third));
    run(&mut s, "drawing.set", json!({"projection": "first"}));
    let at_once = extract(&mut s, sheet).unwrap();
    assert!(at_once.generation > third.generation && !at_once.stale.is_empty());
    assert_eq!(serde_json::to_string(&without_text(at_once.scene.groups.last().unwrap())).unwrap(), block_of(&s, Projection::First));
    let first = settled(&mut s, sheet);
    assert_ne!(block_of(&s, Projection::First), block_of(&s, Projection::Third));
    assert_eq!(bytes(&first.scene), bytes(&built(&s, sheet).scene));
}

/// A sheet with no views is its border alone.
#[test]
fn a_sheet_with_no_views_is_its_border() {
    let (mut s, _, _) = front_and_top();
    let second = run(&mut s, "drawing.sheet.add", json!({}))["sheet"].as_str().and_then(ids::decode).unwrap();
    let SheetScene { scene, views, .. } = built(&s, second);
    assert_eq!((serde_json::to_string(&scene.groups).unwrap(), views.len()), (serde_json::to_string(&border(&s, second)).unwrap(), 0));
}

/// A view in `visible_hidden` is drawn as its snapshot is, its hidden lines dashed under its outline,
/// and a sheet's provider sees the style's change as any other's: the last lines show until the view
/// is drawn again, and it ends at the built scene. The pocket is 40 x 10 mm into the back face, so
/// the front view has four hidden strokes.
#[test]
fn a_view_with_hidden_lines_draws_them_as_its_snapshot_does() {
    let (mut s, sheet, [front, _]) = front_and_top();
    let drawing = ids::encode(s.open_drawing.unwrap());
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "solid.box", json!({"length": 40, "width": 10, "height": 10, "corner": [30, 40, 5], "operation": "cut", "targets": ["Block"]}));
    run(&mut s, "drawing.open", json!({"drawing": drawing.clone()}));
    let outline = settled(&mut s, sheet);
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(front), "set": {"visual_style": "visible_hidden"}}));
    let waiting = extract(&mut s, sheet).unwrap();
    assert_eq!((waiting.stale.contains(&front), bytes(&waiting.scene)), (true, bytes(&outline.scene)));
    let scene = settled(&mut s, sheet).scene;
    assert_eq!(bytes(&scene), bytes(&built(&s, sheet).scene));
    let ours = scene.groups.iter().find(|g| g.object == front).unwrap().clone();
    let dashed = ours.items.iter().filter(|item| matches!(item, Item::Stroke { pattern: Some(_), .. })).count();
    assert_eq!(dashed, 4, "{ours:?}");
    run(&mut s, "drawing.close", json!({}));
    let v = run(
        &mut s,
        "drawing.view.snapshot",
        json!({"view": "front", "scale": "1:2", "position_mm": [150, 150], "scene": true, "visual_style": "visible_hidden"}),
    );
    let snapped: Scene = serde_json::from_value(v["scene"].clone()).unwrap();
    assert!(same_within(&ours, &snapped.groups[0], 1e-9), "{ours:?} {:?}", snapped.groups[0]);
}

/// A sheet's view draws each part's hidden lines by that part's own style. The reference part sits
/// 10 mm behind the model part and 10 mm to its right, so from the front the model hides part of it:
/// those lines are dashed when the reference style draws hidden lines, whatever the model's style,
/// and not when only the model's does (nothing of the model is hidden).
#[test]
fn a_sheets_view_dashes_each_parts_hidden_lines_by_its_own_style() {
    let mut design = Session::default();
    let mut part = |name: &str, body: &str| {
        let occurrence = run(&mut design, "component.create", json!({"name": name}))["occurrence"].as_u64().unwrap();
        run(&mut design, "solid.box", json!({"length": 20, "width": 20, "height": 10, "body_name": body, "operation": "new"}));
        run(&mut design, "component.activate", json!({"component": 0}));
        occurrence
    };
    part("Front", "BodyFront");
    let behind = part("Behind", "BodyBehind");
    run(&mut design, "occurrence.move", json!({"occurrence": behind, "translate": [10, 10, 0]}));
    run(&mut design, "occurrence.bom_type", json!({"occurrence": behind, "bom_type": "reference"}));
    let (mut s, sheet, [front, _]) = front_and_top_of(design);
    let mut dashed = |display: &str, reference: &str| {
        run(&mut s, "drawing.view.edit", json!({"view": ids::encode(front), "set": {"visual_style": display, "reference_style": reference}}));
        let scene = settled(&mut s, sheet).scene;
        let ours = scene.groups.iter().find(|g| g.object == front).unwrap();
        ours.items.iter().filter(|item| matches!(item, Item::Stroke { pattern: Some(_), .. })).count()
    };
    assert!(dashed("visible", "visible_hidden") > 0);
    assert_eq!(dashed("visible_hidden", "visible"), 0);
    assert_eq!(dashed("visible", "visible"), 0);
}

/// A view in a style the paper scene can't draw yet fails with the snapshot's reason, and a view
/// with nothing to measure is a placeholder; each draws nothing, and the sheet's other views and
/// its border are drawn all the same. The command says why for each.
#[test]
fn a_view_that_cant_be_drawn_leaves_out_only_itself() {
    let (mut s, sheet, [front, top]) = front_and_top();
    let status = |s: &Session, id: u64| built(s, sheet).views.into_iter().find(|v| v.view == id).map(|v| v.status);
    let why = "not supported yet: shaded views with lines need the view's exact lines (W06)";
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(top), "set": {"visual_style": "shaded_visible"}}));
    assert_eq!((status(&s, front), status(&s, top)), (Some(Status::Drawn), Some(Status::Failed(why.into()))));
    let objects: Vec<u64> = built(&s, sheet).scene.groups.iter().map(|g| g.object).collect();
    assert_eq!(objects, [sheet, sheet, sheet, front]);
    let reply = run(&mut s, CMD, json!({"sheet": ids::encode(sheet)}));
    let schema = output_schema_for::<SheetSceneResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &reply), "{reply}");
    let failed = reply["views"].as_array().unwrap().iter().find(|v| v["view"] == ids::encode(top)).unwrap().clone();
    assert_eq!((&failed["status"], failed["reason"].as_str().map(|r| r.contains("exact lines"))), (&json!("failed"), Some(true)), "{reply}");
    // Past the walk limit the front view has nothing to measure.
    for level in 0..=MAX_PLACEMENT_DEPTH as u64 + 1 {
        nest(&mut s, level);
    }
    assert_eq!(status(&s, front), Some(Status::Placeholder(Unmeasured::PastWalkLimit)));
    assert_eq!(built(&s, sheet).scene.groups.len(), 3);
}

/// Neither the command nor the provider evaluates the model.
#[test]
fn drawing_a_sheet_evaluates_no_model() {
    let (mut s, sheet, _) = front_and_top();
    let before = s.evaluations;
    run(&mut s, CMD, json!({"sheet": ids::encode(sheet)}));
    settled(&mut s, sheet);
    assert_eq!(s.evaluations, before);
}

/// An unknown sheet and another drawing's sheet are refused, and so is a sheet with no drawing open.
#[test]
fn unknown_and_other_drawings_sheets_are_refused() {
    let (mut s, sheet, _) = front_and_top();
    let refused = |s: &mut Session, named: &str| s.execute(CMD, &json!({"sheet": named})).unwrap_err().to_string();
    assert!(refused(&mut s, "Sheet 9").contains("no sheet `Sheet 9`"));
    run(&mut s, "drawing.create", json!({"name": "Second"}));
    let first = ids::encode(sheet);
    assert!(refused(&mut s, &first).contains(&format!("no sheet `{first}`")));
    assert!(extract(&mut s, sheet).unwrap_err().to_string().contains("the drawing has no sheet"));
    run(&mut s, "drawing.close", json!({}));
    assert!(refused(&mut s, &first).contains("open a drawing first"));
    assert!(extract(&mut s, sheet).is_err() && !step(&mut s));
}

/// A new cache gives the border with both views out of date and draws nothing; each step draws one
/// view, in id order; once nothing is waiting the scene is the built one, byte for byte. The
/// generation goes up each time the scene changes, and only then.
#[test]
fn the_provider_draws_a_view_a_step_until_it_is_the_built_scene() {
    let (mut s, sheet, [front, top]) = front_and_top();
    let order = if front < top { [front, top] } else { [top, front] };
    let whole = built(&s, sheet).scene;
    let first = extract(&mut s, sheet).unwrap();
    assert_eq!(
        (bytes(&first.scene), first.stale.as_slice(), first.views.len()),
        (bytes(&Scene::new(whole.size_mm, whole.groups[..3].to_vec())), &order[..], 0)
    );
    let again = extract(&mut s, sheet).unwrap();
    assert!(Arc::ptr_eq(&again.scene, &first.scene) && again.generation == first.generation);
    assert_eq!(s.drawing_scenes.projections, 0);

    assert!(step(&mut s));
    let one = extract(&mut s, sheet).unwrap();
    let objects: Vec<u64> = one.scene.groups.iter().map(|g| g.object).collect();
    assert_eq!((objects, one.stale.as_slice()), (vec![sheet, sheet, sheet, order[0]], &order[1..]));
    assert!(one.generation > first.generation);
    assert!(!step(&mut s));
    let both = extract(&mut s, sheet).unwrap();
    assert_eq!((bytes(&both.scene), both.stale.len()), (bytes(&whole), 0));
    assert!(both.generation > one.generation);
    assert!(both.views.iter().all(|v| v.status == Status::Drawn) && both.views.len() == 2);
    assert!(!step(&mut s));
    assert_eq!((s.drawing_scenes.projections, extract(&mut s, sheet).unwrap().generation), (2, both.generation));
}

/// Moving a view moves its lines and projects nothing: the top view is lined up with the front
/// one, so both move 50 mm right, and the scene is the built one. Undoing the move projects nothing
/// either and gives back the scene before it, byte for byte.
#[test]
fn a_move_and_its_undo_project_nothing() {
    let (mut s, sheet, [front, _]) = front_and_top();
    let before = settled(&mut s, sheet);
    let projections = s.drawing_scenes.projections;
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(front), "set": {"place": {"at": [200.0, 150.0]}}}));
    let after = extract(&mut s, sheet).unwrap();
    assert_eq!((after.stale.len(), s.drawing_scenes.projections), (0, projections));
    assert!(after.generation > before.generation);
    assert_eq!(bytes(&after.scene), bytes(&built(&s, sheet).scene));
    for (was, now) in before.scene.groups.iter().zip(&after.scene.groups).skip(3) {
        assert!(same_within(now, &moved(was, Vec2::new(50.0, 0.0)), 1e-9), "{now:?}");
    }
    run(&mut s, "edit.undo", json!({}));
    let undone = extract(&mut s, sheet).unwrap();
    assert_eq!((undone.stale.len(), s.drawing_scenes.projections), (0, projections));
    assert_eq!(bytes(&undone.scene), bytes(&before.scene));
}

/// A view's picture moves with its lines: its rectangle by exactly the offset, its pixels and
/// opacity as they were.
#[test]
fn a_moved_group_moves_its_picture() {
    let picture = |min: Vec2, max: Vec2| Item::Picture(Picture { source: 3, rect: Aabb2 { min, max }, opacity: 0.5 });
    let group = Group { object: 7, kind: ObjectKind::View, items: vec![picture(Vec2::new(1.0, 2.0), Vec2::new(11.0, 7.0))] };
    assert_eq!(moved(&group, Vec2::new(50.0, -2.0)).items, [picture(Vec2::new(51.0, 0.0), Vec2::new(61.0, 5.0))]);
}

/// After a model change (going back to a checkpoint taken before a tower was added) both views are
/// out of date and the last scene still shows. Each step draws one view from the model as it is
/// now, and a view listed as up to date always draws as the built scene does.
#[test]
fn after_a_model_change_the_last_scene_shows_until_its_views_are_drawn_again() {
    let (mut s, sheet, _) = front_and_top();
    let drawing = ids::encode(s.open_drawing.unwrap());
    let checkpoint = run(&mut s, "edit.checkpoint", json!({}))["checkpoint"].clone();
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 40, "corner": [40, 60, 0], "body_name": "Tower", "operation": "new"}));
    run(&mut s, "drawing.open", json!({"drawing": drawing}));
    let with_tower = settled(&mut s, sheet);
    run(&mut s, "edit.restore_checkpoint", json!({"checkpoint": checkpoint}));
    let projections = s.drawing_scenes.projections;
    let old = extract(&mut s, sheet).unwrap();
    assert_eq!((old.stale.len(), bytes(&old.scene), old.generation), (2, bytes(&with_tower.scene), with_tower.generation));
    let now = built(&s, sheet).scene;
    let group = |scene: &Scene, id: u64| {
        scene.groups.iter().find(|g| g.object == id && g.kind == ObjectKind::View).map(|g| serde_json::to_string(g).unwrap())
    };
    loop {
        let more = step(&mut s);
        let mid = extract(&mut s, sheet).unwrap();
        for v in mid.views.iter().filter(|v| !mid.stale.contains(&v.view)) {
            assert_eq!(group(&mid.scene, v.view), group(&now, v.view), "{}", v.view);
        }
        if !more {
            break;
        }
    }
    let last = extract(&mut s, sheet).unwrap();
    assert_eq!((last.stale.len(), s.drawing_scenes.projections, bytes(&last.scene)), (0, projections + 2, bytes(&now)));
}

/// The faces a title block's text is set in are part of what its scene is assembled from: the same
/// face keeps the scene and its generation, and another face under the same family name, as when a
/// family is found after it wasn't, is another scene. The text is set once for each drawing and
/// model, not for each view drawn (#4890 F5).
#[test]
fn a_scene_is_for_the_faces_its_title_blocks_text_is_set_in() {
    let (mut s, sheet, _) = front_and_top();
    let block = solidcraft_geom::title_block::default_block();
    run(&mut s, "drawing.sheet.set", json!({"sheet": ids::encode(sheet), "title_block": serde_json::to_value(&block).unwrap()}));
    let again = |s: &mut Session| {
        let d = (**s.drawing("test").unwrap()).clone();
        s.doc_mut().put_drawing(Arc::new(d));
        extract(s, sheet).unwrap()
    };
    let text = |s: &Session| s.drawing_scenes.last.as_ref().unwrap().text.clone();
    extract(&mut s, sheet).unwrap();
    let laid = text(&s);
    let first = settled(&mut s, sheet);
    assert!(Arc::ptr_eq(&laid, &text(&s)), "the views drawn set no text");
    let faces: Vec<[u8; 32]> = first.scene.fonts.iter().map(|font| font.id).collect();
    assert!(!faces.is_empty());
    assert_eq!(laid.faces(), faces);
    assert_eq!(again(&mut s).generation, first.generation, "the same face");
    assert!(!Arc::ptr_eq(&laid, &text(&s)), "set again for the new drawing");

    // Another face under the same family name.
    let last = s.drawing_scenes.last.as_mut().unwrap();
    let mut planted = (*last.text).clone();
    planted.fonts[0].id = [9; 32];
    last.text = Arc::new(planted);
    let other = again(&mut s);
    assert!(other.generation > first.generation, "another face");
    assert_eq!(bytes(&other.scene), bytes(&built(&s, sheet).scene));

    // A family with no face is set in the built-in font, so it is the same scene, until it's found.
    run(&mut s, "drawing.set", json!({"font": "No Such Family"}));
    let missing = extract(&mut s, sheet).unwrap();
    assert_eq!(missing.generation, other.generation);
    assert_eq!(missing.scene.fonts, other.scene.fonts);
}

/// A title block's text is set for the sheet asked for and the model as it is, whichever was last
/// (#4890 F5).
#[test]
fn a_title_blocks_text_follows_its_sheet_and_the_model() {
    use solidcraft_geom::cell::{Content, Format, Part, Source};
    use solidcraft_geom::title_block::CellKind;
    let (mut s, first, _) = front_and_top();
    let drawing = ids::encode(s.open_drawing.unwrap());
    let source = s.drawing("test").unwrap().sources[0].id;
    let second = run(&mut s, "drawing.sheet.add", json!({}))["sheet"].as_str().and_then(ids::decode).unwrap();
    let mut block = solidcraft_geom::title_block::default_block();
    let mass = block.cells.iter_mut().find_map(|c| if let CellKind::Text(text) = &mut c.kind { Some(text) } else { None }).unwrap();
    mass.content = Content(vec![Part::Field { source: Source::Component, key: "mass".into(), format: Format::default() }]);
    let block = serde_json::to_value(&block).unwrap();
    let documents = json!({"source": ids::encode(source), "component": 0});
    run(&mut s, "drawing.sheet.set", json!({"sheet": ids::encode(first), "title_block": block, "documents": documents}));
    run(&mut s, "drawing.sheet.set", json!({"sheet": ids::encode(second), "title_block": block, "title_block_overrides": {"title": "Second"}}));
    let checkpoint = run(&mut s, "edit.checkpoint", json!({}))["checkpoint"].clone();
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 40, "corner": [40, 60, 0], "body_name": "Tower", "operation": "new_body"}));
    run(&mut s, "drawing.open", json!({"drawing": drawing}));

    // Back and forth between the sheets, each is its own.
    for sheet in [first, second, first] {
        assert_eq!(bytes(&settled(&mut s, sheet).scene), bytes(&built(&s, sheet).scene), "{sheet}");
    }
    let shown = |s: &Session| s.drawing_scenes.last.as_ref().unwrap().cells.clone();
    let with_tower = shown(&s);
    run(&mut s, "edit.restore_checkpoint", json!({"checkpoint": checkpoint}));
    assert_eq!(bytes(&settled(&mut s, first).scene), bytes(&built(&s, first).scene));
    assert_ne!(shown(&s), with_tower, "the mass read from the model as it is");
}

/// The window's sheet never waits for the search for a font to fall back to: a title block text in
/// a script the drawing's font lacks is off the sheet until the search answers, `step` says there
/// is more to do though no view is waiting, and the settled sheet is the one `build` gives (which
/// waits). The character is this test's alone, so no other test has searched for it first.
#[test]
fn a_title_blocks_text_waits_for_its_font_off_the_windows_thread() {
    let (mut s, sheet, _) = front_and_top();
    let block = serde_json::to_value(solidcraft_geom::title_block::default_block()).unwrap();
    run(&mut s, "drawing.sheet.set", json!({"sheet": ids::encode(sheet), "title_block": block}));
    settled(&mut s, sheet);
    run(&mut s, "drawing.sheet.set", json!({"sheet": ids::encode(sheet), "title_block_overrides": {"title": "Plate \u{30fa}"}}));
    let waiting = extract(&mut s, sheet).unwrap();
    assert!(s.drawing_scenes.last.as_ref().unwrap().pending, "the window's sheet asked, not waited");
    assert!(waiting.waiting_for_fonts, "the answer says its text is waiting");
    // No view is out of date: text alone is no work for a step, so a caller that steps until
    // there's nothing left (a line pick does) never waits for the search.
    assert!(!step(&mut s), "a step has nothing to do for text");
    assert!(line_at(&mut s, sheet, Vec2::new(150.0, 150.0), 1.0).is_ok());
    assert_eq!(bytes(&settled(&mut s, sheet).scene), bytes(&built(&s, sheet).scene));
    assert!(!s.drawing_scenes.last.as_ref().unwrap().pending);
}

/// A dimension's number in a script the drawing's font lacks is read again until the search for its
/// font answers, then drawn as `build` draws it. The character is this test's alone.
#[test]
fn a_dimensions_text_waits_for_its_font_off_the_windows_thread() {
    let (mut s, sheet, [front, _]) = front_and_top();
    settled(&mut s, sheet);
    let p = json!({"view": ids::encode(front), "kind": {"length": {"edge": [50, 0, 20]}}});
    let added = run(&mut s, "drawing.dimension.add", p)["dimension"].as_str().and_then(ids::decode).unwrap();
    // Its text through edit, which lays nothing out: add asks for a font itself (it never waits), and
    // the read this test watches must be the first to ask.
    run(&mut s, "drawing.dimension.edit", json!({"dimension": ids::encode(added), "set": {"override_text": "\u{30f7} 100"}}));
    extract(&mut s, sheet).unwrap();
    step(&mut s);
    assert!(s.drawing_scenes.dimensions[&added].drawn.pending, "the window's sheet asked, not waited");
    let first = extract(&mut s, sheet).unwrap();
    assert!(first.waiting_for_fonts, "the answer says its text is waiting");
    assert!(!step(&mut s), "it is read again by extract, not by a step");
    // Still waiting, nothing changed: the sheet isn't put together again.
    let again = extract(&mut s, sheet).unwrap();
    if again.waiting_for_fonts {
        assert_eq!(again.generation, first.generation);
    }
    assert_eq!(bytes(&settled(&mut s, sheet).scene), bytes(&built(&s, sheet).scene));
    assert!(!s.drawing_scenes.dimensions[&added].drawn.pending);
}

/// A view drawn again takes its dimensions with it, through the view's job: that read asks for a
/// font too, as the window's other reads do, so a step never waits for the search. The character is
/// this test's alone.
#[test]
fn a_view_drawn_again_reads_its_dimensions_text_without_waiting() {
    let (mut s, sheet, [front, _]) = front_and_top();
    settled(&mut s, sheet);
    let p = json!({"view": ids::encode(front), "kind": {"length": {"edge": [50, 0, 20]}}});
    let added = run(&mut s, "drawing.dimension.add", p)["dimension"].as_str().and_then(ids::decode).unwrap();
    // Its text through edit, which lays nothing out: add asks for a font itself (it never waits), and
    // the read this test watches must be the first to ask.
    run(&mut s, "drawing.dimension.edit", json!({"dimension": ids::encode(added), "set": {"override_text": "\u{30f8} 100"}}));
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(front), "set": {"scale": "1:4"}}));
    extract(&mut s, sheet).unwrap();
    // Both views wait (the top one is projected from the front one): step until the front view's
    // job has read its dimension.
    let mut waiting = true;
    while !s.drawing_scenes.dimensions.contains_key(&added) {
        assert!(waiting, "every view is drawn and the dimension wasn't read");
        waiting = step(&mut s);
    }
    assert!(s.drawing_scenes.dimensions[&added].drawn.pending, "the view's job asked, not waited");
    assert_eq!(bytes(&settled(&mut s, sheet).scene), bytes(&built(&s, sheet).scene));
    assert!(!s.drawing_scenes.dimensions[&added].drawn.pending);
}

/// Adding a dimension whose number needs a font to fall back to never waits for the search, as add
/// and its dry run run on the window thread: the reply has no layout and says its text waits. Once the
/// sheet has drawn it, inspect has the layout. The character is this test's alone.
#[test]
fn adding_a_dimension_never_waits_for_its_texts_font() {
    let (mut s, sheet, [front, _]) = front_and_top();
    settled(&mut s, sheet);
    let p = json!({"view": ids::encode(front), "kind": {"length": {"edge": [50, 0, 20]}}, "override_text": "\u{30f9} 100"});
    let added = run(&mut s, "drawing.dimension.add", p);
    let waits = "its text waits for a font to fall back to; drawing.scene draws it when one is found";
    assert!(added.get("layout").is_none() && added["layout_why"] == json!(waits), "{added}");
    settled(&mut s, sheet);
    // Found, or known to be in no font: either way the answer is in, and the layout with it.
    let inspected = run(&mut s, "drawing.dimension.inspect", json!({"dimension": added["dimension"]}));
    assert!(inspected.get("layout").is_some() && inspected["layout_why"] != json!(waits), "{inspected}");
}

/// The model's state alone changing, the design and the sheet as they were, sets the title
/// block's text again: its mass is read from the state, not the design.
#[test]
fn a_title_blocks_text_follows_the_model_state_alone() {
    use solidcraft_geom::cell::{Content, Format, Part, Source};
    use solidcraft_geom::title_block::CellKind;
    let (mut s, sheet, _) = front_and_top();
    let drawing = ids::encode(s.open_drawing.unwrap());
    let source = s.drawing("test").unwrap().sources[0].id;
    let mut block = solidcraft_geom::title_block::default_block();
    let mass = block.cells.iter_mut().find_map(|c| if let CellKind::Text(text) = &mut c.kind { Some(text) } else { None }).unwrap();
    mass.content = Content(vec![Part::Field { source: Source::Component, key: "mass".into(), format: Format::default() }]);
    let documents = json!({"source": ids::encode(source), "component": 0});
    run(&mut s, "drawing.sheet.set", json!({"sheet": ids::encode(sheet), "title_block": block, "documents": documents}));
    let without_tower = s.model.clone();
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 40, "corner": [40, 60, 0], "body_name": "Tower", "operation": "new_body"}));
    run(&mut s, "drawing.open", json!({"drawing": drawing}));
    settled(&mut s, sheet);
    let shown = |s: &Session| s.drawing_scenes.last.as_ref().unwrap().cells.clone();
    let with_tower = shown(&s);

    // The state from before the tower, the design unchanged.
    s.model = without_tower;
    assert_eq!(bytes(&settled(&mut s, sheet).scene), bytes(&built(&s, sheet).scene));
    assert_ne!(shown(&s), with_tower, "the mass read from the model's state as it is");
}

/// Closing the drawing drops its scenes and the world states they hold, and keeps each view's lines.
/// Opened again with nothing changed, its views show those lines, up to date, and none is drawn:
/// only its dimensions are read again.
#[test]
fn closing_the_drawing_keeps_its_lines_for_when_it_opens_again() {
    let (mut s, sheet, [front, _]) = front_and_top();
    top_edge(&mut s, front);
    let drawing = ids::encode(s.open_drawing.unwrap());
    let before = settled(&mut s, sheet);
    run(&mut s, "drawing.close", json!({}));
    let cache = &s.drawing_scenes;
    assert!(cache.views.is_empty() && cache.dimensions.is_empty() && cache.last.is_none() && cache.shown.is_none() && cache.drawing.is_none());
    assert!(!step(&mut s));
    run(&mut s, "drawing.open", json!({"drawing": drawing}));
    let reopened = extract(&mut s, sheet).unwrap();
    // A new scene, so a new number: the page can't take it for the one it showed before.
    assert!(reopened.stale.is_empty() && reopened.generation > before.generation, "{:?}", reopened.stale);
    assert_eq!(reopened.stale_dimensions.len(), 1);
    let after = settled(&mut s, sheet);
    assert_eq!((bytes(&after.scene), s.drawing_scenes.projections), (bytes(&before.scene), 0));
}

/// Opened again with nothing changed, before any sheet is drawn, `drawing.status` says its views are
/// current: the lines kept for them show what the model shows, so none waits to be drawn.
#[test]
fn opened_again_its_status_says_the_kept_lines_are_current() {
    let (mut s, sheet, _) = front_and_top();
    let drawing = ids::encode(s.open_drawing.unwrap());
    settled(&mut s, sheet);
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "drawing.open", json!({"drawing": drawing}));
    let (reply, words) = status_of(&mut s);
    assert!(all_are(&words, "current") && reply["waiting"] == json!(0), "{reply}");
    assert!(extract(&mut s, sheet).unwrap().stale.is_empty());
    assert_eq!(s.drawing_scenes.projections, 0);
}

/// A change that leaves every body where it was and as it was draws no view: a sketch (a new feature,
/// so a new world state, that adds no body), and a model edit taken back before its views were drawn
/// again.
#[test]
fn a_change_that_shows_the_same_draws_no_view() {
    let (mut s, sheet, _) = front_and_top();
    let drawing = json!({"drawing": ids::encode(s.open_drawing.unwrap())});
    let before = settled(&mut s, sheet);
    let state = s.world_state().unwrap();
    let checkpoint = run(&mut s, "edit.checkpoint", json!({}))["checkpoint"].clone();
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Note"}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "drawing.open", drawing.clone());
    assert!(!Arc::ptr_eq(&state, &s.world_state().unwrap()), "a new world state");
    assert!(extract(&mut s, sheet).unwrap().stale.is_empty());

    run(&mut s, "drawing.close", json!({}));
    let block = s.doc.features[0].id;
    run(&mut s, "timeline.edit", json!({"feature": block.to_string(), "set": {"height": 40}}));
    run(&mut s, "drawing.open", drawing);
    assert_eq!(extract(&mut s, sheet).unwrap().stale.len(), 2, "40 tall shows otherwise");
    run(&mut s, "edit.restore_checkpoint", json!({"checkpoint": checkpoint}));
    // A step or a job straight after the change, with no extract between, finds nothing to draw either.
    assert!(begin(&mut s).is_none());
    assert!(!step(&mut s));
    let back = extract(&mut s, sheet).unwrap();
    assert!(back.stale.is_empty(), "{:?}", back.stale);
    assert_eq!((bytes(&back.scene), s.drawing_scenes.projections), (bytes(&before.scene), 0));
}

/// A body renamed while the drawing was closed draws its views again, so a press on a line names it
/// as it is called now.
#[test]
fn a_renamed_body_draws_again() {
    let (mut s, sheet, _) = front_and_top();
    let drawing = json!({"drawing": ids::encode(s.open_drawing.unwrap())});
    settled(&mut s, sheet);
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "body.rename", json!({"body": "Block", "name": "Brick"}));
    run(&mut s, "drawing.open", drawing);
    assert_eq!(extract(&mut s, sheet).unwrap().stale.len(), 2);
    settled(&mut s, sheet);
    assert_eq!(line_under(&mut s, sheet, [150.0, 155.2], 0.5)["body"], json!("Brick"));
}

/// A part made a reference while the drawing was closed stands where it stood, but draws as a
/// reference part now: its views draw again.
#[test]
fn a_part_made_reference_draws_again() {
    let (design, over) = under_and_over();
    let (mut s, sheet, _) = front_and_top_of(design);
    let drawing = json!({"drawing": ids::encode(s.open_drawing.unwrap())});
    settled(&mut s, sheet);
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "occurrence.bom_type", json!({"occurrence": over, "bom_type": "reference"}));
    run(&mut s, "drawing.open", drawing);
    assert_eq!(extract(&mut s, sheet).unwrap().stale.len(), 2);
}

/// [`two_cubes`] with an exploded view, Apart, moving the second 30 mm further along x, and a drawing
/// of them, open, with no title block: two assembled front views and one drawing Apart, each at 1:1.
/// The session, the sheet, the two assembled views, the exploded one and Apart's id.
fn two_assembled_one_apart() -> (Session, u64, [u64; 3], u64) {
    let (mut s, [_, second]) = two_cubes();
    let apart =
        run(&mut s, "explode.create", json!({"name": "Apart", "moves": [{"occurrence": second, "translate": [30, 0, 0]}]}))["view"].as_u64().unwrap();
    run(&mut s, "drawing.create", json!({}));
    let mut d = (**s.drawing("test").unwrap()).clone();
    Arc::make_mut(&mut d.sheets[0]).set_title_block(None);
    let mut id_source = solidcraft_doc::ids::IdSource::seeded(13048);
    let (sheet, source) = (d.sheets[0].id, d.sources[0].id);
    let mut add = |d: &mut Drawing, at: [f64; 2], exploded: Option<Exploded>| {
        let id = d.allocate_id(&mut id_source).unwrap();
        let settings = ViewOverrides { scale: Some("1:1".parse().unwrap()), exploded, ..ViewOverrides::default() };
        let kind = ViewKind::Base { source, view: ViewDirection::Named(NamedView::Front) };
        d.add_views([Arc::new(View { id, sheet, kind, place: ViewPlace::At(at), settings, reference_shown_percent: 0.0, label: None })]);
        id
    };
    let views = [
        add(&mut d, [60.0, 150.0], None),
        add(&mut d, [60.0, 250.0], Some(Exploded::Assembled)),
        add(&mut d, [250.0, 150.0], Some(Exploded::View(apart))),
    ];
    d.validate().unwrap();
    s.doc_mut().put_drawing(Arc::new(d));
    (s, sheet, views, apart)
}

/// An exploded view is up to date by what it draws, not by what the design places: moving its parts
/// further apart draws it again and leaves the assembled views as they were; moving them back to
/// where they were drawn draws nothing.
#[test]
fn an_exploded_views_moves_draw_it_again_and_only_it() {
    let (mut s, sheet, [_, _, exploded], _) = two_assembled_one_apart();
    let drawing = json!({"drawing": ids::encode(s.open_drawing.unwrap())});
    settled(&mut s, sheet);
    let moves = |s: &mut Session, x: f64| {
        run(s, "drawing.close", json!({}));
        let doc = s.doc_mut();
        let view = doc.assembly.exploded.first_mut().unwrap();
        view.moves[0].1 = solidcore_record::Vec3::new(x, 0.0, 0.0);
    };
    moves(&mut s, 60.0);
    run(&mut s, "drawing.open", drawing.clone());
    assert_eq!(extract(&mut s, sheet).unwrap().stale, vec![exploded]);
    settled(&mut s, sheet);
    moves(&mut s, 60.0);
    run(&mut s, "drawing.open", drawing);
    assert!(extract(&mut s, sheet).unwrap().stale.is_empty(), "the same moves draw the same");
}

/// A sheet read makes one copy of the session for each exploded view its views draw, shared by every
/// view that draws it, and none for the views that draw the design as it is placed.
#[test]
fn a_sheet_makes_one_copy_for_each_exploded_view_it_draws() {
    let (mut s, sheet, [assembled, own_assembled, exploded], _) = two_assembled_one_apart();
    let mut d = (**s.drawing("test").unwrap()).clone();
    let id = d.allocate_id(&mut solidcraft_doc::ids::IdSource::seeded(13049)).unwrap();
    let mut second = (*d.views.iter().find(|v| v.id == exploded).unwrap().as_ref()).clone();
    (second.id, second.place) = (id, ViewPlace::At([250.0, 250.0]));
    d.add_views([Arc::new(second)]);
    d.validate().unwrap();
    s.doc_mut().put_drawing(Arc::new(d.clone()));
    let mut sessions = crate::drawing_view::ViewSessions::new(&s);
    for view in [assembled, own_assembled, exploded, id] {
        let resolved = d.resolve_view(view).unwrap();
        sessions.of(&resolved).unwrap();
    }
    assert_eq!(sessions.copies(), 1);
    built(&s, sheet);
}

/// A sheet read makes one copy of the session for each state of the assembly its views draw: two views
/// of the hinge turned up share one, a third turned up and exploded has its own, and the view of the
/// design's placement needs none.
#[test]
fn a_sheet_makes_one_copy_for_each_choice_it_draws() {
    let (mut s, designed, _) = hinge_pair();
    let drawing = json!({"drawing": ids::encode(s.open_drawing.unwrap())});
    run(&mut s, "drawing.close", json!({}));
    add_a_pin(&mut s);
    let leaf = s.doc.occurrences.iter().find(|o| o.name.starts_with("Leaf")).map(|o| o.id).unwrap();
    run(&mut s, "explode.create", json!({"name": "Apart", "moves": [{"occurrence": leaf, "translate": [0, 30, 0]}]}));
    run(&mut s, "drawing.open", drawing);
    let sheet = s.drawing("test").unwrap().sheets[0].id;
    let view = |s: &mut Session, at: [f64; 2], exploded: &str| {
        let p = json!({"sheet": ids::encode(sheet), "view": "top", "scale": "1:1", "position_mm": at, "exploded": exploded,
                       "pose": [{"joint": "Hinge", "values": [90]}]});
        ids::decode(run(s, "drawing.view.base", p)["view"].as_str().unwrap()).unwrap()
    };
    let posed = [view(&mut s, [60.0, 250.0], "assembled"), view(&mut s, [150.0, 250.0], "assembled"), view(&mut s, [250.0, 250.0], "Apart")];
    let d = s.drawing("test").unwrap().clone();
    let mut sessions = crate::drawing_view::ViewSessions::new(&s);
    for view in [designed].into_iter().chain(posed) {
        sessions.of(&d.resolve_view(view).unwrap()).unwrap();
    }
    assert_eq!(sessions.copies(), 2);
    built(&s, sheet);
}

/// An exploded view's copy holds its moves in its own design (#13048), so every reader of where the
/// parts are sees them. Its bodies are placed as `explode.show`'s pending moves place them, body for
/// body, nothing is left pending, and the design keeps its placements.
#[test]
fn an_exploded_copy_holds_its_moves_in_its_own_design() {
    use crate::cmd::motion::{explode_moves, exploded_at};
    let (s, _, [.., apart], _) = two_assembled_one_apart();
    let placed = s.doc.occurrences.clone();
    let view = s.drawing("test").unwrap().resolve_view(apart).unwrap();
    let copy = crate::drawing_view::copy_for(&s, &Choice::of(&view)).unwrap().session;
    let mut shown = s.scratch();
    shown.pending_moves = exploded_at(&explode_moves(&shown, &s.doc.assembly.exploded[0]), 1.0);
    let boxes = |s: &Session| -> Vec<(String, String)> {
        s.world_state().unwrap().bodies.iter().map(|b| (b.name.clone(), format!("{:?}", b.body.exact_bounds().unwrap()))).collect()
    };
    assert_eq!(boxes(&copy), boxes(&shown));
    assert_ne!(copy.doc.occurrences, placed, "the moves are in the copy's design");
    assert!(copy.pending_moves.is_empty());
    assert_eq!(s.doc.occurrences, placed);
}

/// Two views at one step of a motion study share one copy, and a view at another step has its own.
#[test]
fn views_at_one_study_step_share_one_copy() {
    let (mut s, designed, _) = hinge_pair();
    let drawing = json!({"drawing": ids::encode(s.open_drawing.unwrap())});
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "motion.study", json!({"name": "Open", "steps": 10, "keys": [{"joint": "Hinge", "points": [[0, 0], [10, 90]]}]}));
    run(&mut s, "drawing.open", drawing);
    let sheet = s.drawing("test").unwrap().sheets[0].id;
    let view = |s: &mut Session, at: [f64; 2], step: u32| {
        let p = json!({"sheet": ids::encode(sheet), "view": "top", "scale": "1:1", "position_mm": at, "pose": {"study": "Open", "step": step}});
        ids::decode(run(s, "drawing.view.base", p)["view"].as_str().unwrap()).unwrap()
    };
    let at_steps = [view(&mut s, [60.0, 250.0], 4), view(&mut s, [150.0, 250.0], 4), view(&mut s, [250.0, 250.0], 7)];
    let d = s.drawing("test").unwrap().clone();
    let mut sessions = crate::drawing_view::ViewSessions::new(&s);
    for view in [designed].into_iter().chain(at_steps) {
        sessions.of(&d.resolve_view(view).unwrap()).unwrap();
    }
    assert_eq!(sessions.copies(), 2);
    built(&s, sheet);
}

/// How wide view `id` is on sheet `sheet` built at once, round what it draws, in paper millimetres.
fn drawn_width(s: &Session, sheet: u64, id: u64) -> f64 {
    built(s, sheet).objects.iter().find(|o| o.object == id).map_or(f64::NAN, |o| o.bounds_mm.size().x)
}

/// Views of one row of the configuration table share one copy, and a view of the design's active row,
/// which the design matches, draws the design itself, with no copy (#13048). Once `L` is edited by
/// hand the design no longer matches its active row, so a view of that row draws the row, from a copy
/// of its own.
#[test]
fn views_of_one_row_share_one_copy_and_the_active_row_needs_none() {
    let (mut s, sheet, [front, top], _) = configured_front_and_top();
    let default = s.doc.configs.active_row().map(|row| row.id).unwrap();
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(front), "set": {"configuration": "Long"}}));
    let p = json!({"sheet": ids::encode(sheet), "view": "front", "scale": "1:2", "position_mm": [300, 80], "configuration": default});
    let own_row = ids::decode(run(&mut s, "drawing.view.base", p)["view"].as_str().unwrap()).unwrap();
    let copies = |s: &Session| {
        let d = s.drawing("test").unwrap().clone();
        let mut sessions = crate::drawing_view::ViewSessions::new(s);
        for view in [front, top, own_row] {
            sessions.of(&d.resolve_view(view).unwrap()).unwrap();
        }
        sessions.copies()
    };
    assert_eq!(copies(&s), 1);
    let widths = |s: &Session| [front, top, own_row].map(|view| drawn_width(s, sheet, view));
    assert!(widths(&s).iter().zip([60.0, 60.0, 50.0]).all(|(got, want)| (got - want).abs() < 1e-6), "{:?}", widths(&s));
    in_the_model(&mut s, "parameters.change", json!({"name": "L", "expression": "110 mm"}));
    assert_eq!(copies(&s), 2, "the design no longer matches Default");
    assert!((drawn_width(&s, sheet, own_row) - 50.0).abs() < 1e-6, "the row, not the design's 55: {:?}", widths(&s));
}

/// A view of a row the design no longer has is its placeholder, and so are the views that follow it:
/// each can still be picked, by the 20 x 20 mm round its centre (#13048).
#[test]
fn a_view_of_a_deleted_row_is_an_object_at_its_placeholder() {
    let (mut s, sheet, [front, top], _) = configured_front_and_top();
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(front), "set": {"configuration": "Long"}}));
    in_the_model(&mut s, "config.row", json!({"name": "Long", "delete": true}));
    let scene = built(&s, sheet);
    for (view, min, max) in [(front, [140.0, 140.0], [160.0, 160.0]), (top, [140.0, 210.0], [160.0, 230.0])] {
        let status = scene.views.iter().find(|v| v.view == view).map(|v| v.status.clone());
        assert_eq!(status, Some(Status::Placeholder(Unmeasured::NoConfiguration)));
        let object = scene.objects.iter().find(|o| o.object == view).unwrap();
        assert!(near(&object.bounds_mm, min, max) && object.boundary_mm == Some(object.bounds_mm), "{object:?}");
    }
}

/// A view of a configuration of its own draws from a copy that evaluates the design, so only its job
/// makes that copy (#13048). Behind after `L` is edited by hand, though it would show the same, the
/// window's paths (the sheet extracted a hundred times, the drawing's standing, the job asked for)
/// copy nothing and leave it behind; its job draws it.
#[test]
fn only_its_job_makes_a_configuration_views_copy() {
    let (mut s, sheet, [front, top], _) = configured_front_and_top();
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(front), "set": {"configuration": "Long"}}));
    settled(&mut s, sheet);
    in_the_model(&mut s, "parameters.change", json!({"name": "L", "expression": "110 mm"}));
    crate::tests::doc_copies::take();
    for _ in 0..100 {
        extract(&mut s, sheet).unwrap();
    }
    let d = s.drawing("test").unwrap().clone();
    standing(&mut s, &d);
    let job = begin(&mut s).unwrap();
    assert_eq!(crate::tests::doc_copies::take(), 0, "nothing copied on the window's thread");
    let mut behind = vec![front, top];
    behind.sort_unstable();
    assert_eq!(extract(&mut s, sheet).unwrap().stale, behind);
    assert!(finish(&mut s, job.run()));
    assert!(crate::tests::doc_copies::take() > 0, "the job made it");
    let answer = settled(&mut s, sheet);
    assert!(answer.stale.is_empty() && answer.views.iter().all(|v| v.status == Status::Drawn));
    assert_eq!(bytes(&answer.scene), bytes(&built(&s, sheet).scene));
}

/// A dimension on a view of a configuration of its own, edited while its view is up to date, isn't read
/// again on the window's paths, as its view's copy evaluates the design ([`Choice::evaluates`]): the
/// view is handed back to its job, which reads it with its lines, on the row: the top front edge
/// measures Long's 120 mm (#13048).
#[test]
fn a_configuration_views_dimension_is_read_again_by_its_job() {
    let (mut s, sheet, [front, _], _) = configured_front_and_top();
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(front), "set": {"configuration": "Long"}}));
    let p = json!({"view": ids::encode(front), "kind": {"length": {"edge": [50, 0, 20]}}});
    let added = run(&mut s, "drawing.dimension.add", p)["dimension"].clone();
    settled(&mut s, sheet);
    run(&mut s, "drawing.dimension.edit", json!({"dimension": added, "set": {"offset_mm": 15}}));
    crate::tests::doc_copies::take();
    extract(&mut s, sheet).unwrap();
    assert!(step(&mut s), "the view's job is still to run");
    assert_eq!(crate::tests::doc_copies::take(), 0, "nothing copied on the window's paths");
    assert!(extract(&mut s, sheet).unwrap().stale.contains(&front), "handed back to its job");
    let answer = settled(&mut s, sheet);
    assert!(answer.stale.is_empty());
    assert_eq!(bytes(&answer.scene), bytes(&built(&s, sheet).scene));
    let read = run(&mut s, "drawing.dimension.inspect", json!({"dimension": added}));
    assert!(read["value"].as_f64().is_some_and(|value| (value - 120.0).abs() < 1e-9), "{read}");
}

/// A dimension on a view of a configuration of its own whose text waits for its font is drawn again by
/// its view's job once the font is found, never on the window's paths, which would copy the design
/// (#13048). The character is this test's alone.
#[test]
fn a_configuration_views_waiting_text_is_drawn_by_its_job() {
    let (mut s, sheet, [front, _], _) = configured_front_and_top();
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(front), "set": {"configuration": "Long"}}));
    let p = json!({"view": ids::encode(front), "kind": {"length": {"edge": [50, 0, 20]}}});
    let added = run(&mut s, "drawing.dimension.add", p)["dimension"].clone();
    let id = added.as_str().and_then(ids::decode).unwrap();
    settled(&mut s, sheet);
    // Its text through edit, which lays nothing out, so the view's job is the first to ask for its font.
    run(&mut s, "drawing.dimension.edit", json!({"dimension": added, "set": {"override_text": "\u{30f6} 100"}}));
    extract(&mut s, sheet).unwrap();
    assert!(step(&mut s), "handed back to its job");
    step(&mut s);
    assert!(s.drawing_scenes.dimensions[&id].drawn.pending, "the view's job asked, not waited");
    crate::tests::doc_copies::take();
    let handed = extract(&mut s, sheet).unwrap();
    assert_eq!(crate::tests::doc_copies::take(), 0, "nothing copied on the window's paths");
    assert!(handed.stale.contains(&front), "handed back to its job again");
    // Its view, out of date now, no longer counts its text, which still waits: a page that stepped before it
    // extracted would otherwise stop with nothing out of date and nothing waiting.
    assert!(handed.waiting_for_fonts, "the answer still says its text is waiting");
    let answer = settled(&mut s, sheet);
    assert!(!s.drawing_scenes.dimensions[&id].drawn.pending, "its job set the text");
    assert_eq!(bytes(&answer.scene), bytes(&built(&s, sheet).scene));
    let read = run(&mut s, "drawing.dimension.inspect", json!({"dimension": added}));
    assert!(read["value"].as_f64().is_some_and(|value| (value - 120.0).abs() < 1e-9), "{read}");
}

/// A panic while a view's job makes its copy of another configuration fails the view, and the view
/// that follows it, saying why; each is then up to date, so the next tick asks for nothing and the
/// copy isn't made again every frame (#13048).
#[test]
fn a_copy_that_panics_in_its_job_fails_its_view_and_is_not_asked_for_again() {
    let (mut s, sheet, [front, top], _) = configured_front_and_top();
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(front), "set": {"configuration": "Long"}}));
    let answer = copy_panics::with(|| settled(&mut s, sheet));
    for view in [front, top] {
        let status = answer.views.iter().find(|v| v.view == view).map(|v| v.status.clone());
        assert_eq!(status, Some(Status::Failed(COPY_PANICKED.into())), "{view}");
    }
    assert!(answer.stale.is_empty() && begin(&mut s).is_none() && !step(&mut s), "nothing is asked for again");
}

/// A view's copy that panics after evaluating its row leaves only whole results in the evaluation cache
/// the design shares with it: the design then activates that row (with the drawing closed, as a model
/// edit has it) and draws it as a session whose copy never panicked does, Long's 120 mm at 1:2 (#13048).
#[test]
fn a_copy_that_panics_after_evaluating_leaves_the_design_drawing_its_row_right() {
    let width_after = |panics: bool| {
        let (mut s, sheet, [front, top], _) = configured_front_and_top();
        run(&mut s, "drawing.view.edit", json!({"view": ids::encode(front), "set": {"configuration": "Long"}}));
        if panics {
            copy_panics::with(|| settled(&mut s, sheet));
        } else {
            settled(&mut s, sheet);
        }
        let drawing = json!({"drawing": ids::encode(s.open_drawing.unwrap())});
        run(&mut s, "drawing.close", json!({}));
        run(&mut s, "config.activate", json!({"row": "Long"}));
        run(&mut s, "drawing.open", drawing);
        [drawn_width(&s, sheet, front), drawn_width(&s, sheet, top)]
    };
    let (panicked, fresh) = (width_after(true), width_after(false));
    assert_eq!(panicked, fresh);
    assert!(panicked.iter().all(|w| (w - 60.0).abs() < 1e-6), "{panicked:?}");
}

/// A view of a row whose features fail to build says so beside it once its job has drawn it, in the
/// words the sheet built at once says (#13048).
#[test]
fn a_views_job_says_its_row_rebuilds_with_errors() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "R", "expression": "5 mm"}));
    run(&mut s, "solid.box", json!({"length": 100, "width": 50, "height": 20, "body_name": "Block"}));
    run(&mut s, "solid.sphere", json!({"radius": "R"}));
    run(&mut s, "config.configure", json!({"params": ["R"]}));
    run(&mut s, "config.row", json!({"name": "Broken", "values": {"R": "-1 mm"}}));
    let (mut s, sheet, [front, _]) = front_and_top_of(s);
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(front), "set": {"configuration": "Broken"}}));
    let says = Some("its configuration `Broken` rebuilds with 1 failing feature".to_string());
    let answer = settled(&mut s, sheet);
    assert_eq!(answer.views.iter().find(|v| v.view == front).map(|v| v.why.clone()), Some(says.clone()));
    assert_eq!(built(&s, sheet).views.iter().find(|v| v.view == front).map(|v| v.why.clone()), Some(says));
}

/// A view of a row that fails to build, in a position whose joint is gone, says both, the row first,
/// joined as `config.activate` joins its errors (#13048).
#[test]
fn a_views_row_and_position_reasons_are_joined() {
    let (mut s, view, _) = hinge_pair();
    let sheet = s.drawing("test").unwrap().sheets[0].id;
    in_the_model(&mut s, "parameters.add", json!({"name": "R", "expression": "5 mm"}));
    in_the_model(&mut s, "solid.sphere", json!({"radius": "R"}));
    in_the_model(&mut s, "config.configure", json!({"params": ["R"]}));
    in_the_model(&mut s, "config.row", json!({"name": "Broken", "values": {"R": "-1 mm"}}));
    let set = json!({"configuration": "Broken", "pose": [{"joint": "Hinge", "values": [0]}]});
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(view), "set": set}));
    in_the_model(&mut s, "joint.delete", json!({"joint": "Hinge"}));
    let says = "its configuration `Broken` rebuilds with 1 failing feature; its position names 1 joint the design no longer has";
    assert_eq!(built(&s, sheet).views.iter().find(|v| v.view == view).and_then(|v| v.why.clone()).as_deref(), Some(says));
}

/// A view in a position of its own is up to date by what it draws: driving the hinge in the design
/// draws the view of the design's placement again and leaves the posed view as it was, and editing the
/// posed view's values draws it again and only it.
#[test]
fn a_driven_joint_redraws_a_designed_view_but_not_a_posed_one() {
    let (mut s, designed, _) = hinge_pair();
    let sheet = s.drawing("test").unwrap().sheets[0].id;
    let pose = json!([{"joint": "Hinge", "values": [90]}]);
    let p = json!({"sheet": ids::encode(sheet), "view": "top", "scale": "1:1", "position_mm": [150, 250], "pose": pose});
    let posed = ids::decode(run(&mut s, "drawing.view.base", p)["view"].as_str().unwrap()).unwrap();
    let drawing = json!({"drawing": ids::encode(s.open_drawing.unwrap())});
    settled(&mut s, sheet);
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "joint.drive", json!({"joint": "Hinge", "values": [45]}));
    run(&mut s, "drawing.open", drawing);
    assert_eq!(extract(&mut s, sheet).unwrap().stale, vec![designed]);
    settled(&mut s, sheet);
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(posed), "set": {"pose": [{"joint": "Hinge", "values": [30]}]}}));
    assert_eq!(extract(&mut s, sheet).unwrap().stale, vec![posed]);
}

/// A deleted joint that leaves a posed view's picture as it was keeps the view's lines, and says why on
/// the sheet: the view takes the new model without drawing again, and the reason with it.
#[test]
fn a_gone_joint_keeps_a_posed_views_lines_and_says_why() {
    let (mut s, view, _) = hinge_pair();
    let sheet = s.drawing("test").unwrap().sheets[0].id;
    // At the value the design has, so the leaf lies where it would without the joint.
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(view), "set": {"pose": [{"joint": "Hinge", "values": [0]}]}}));
    let why = |answer: &SheetView| answer.views.iter().find(|v| v.view == view).map(|v| v.why.clone());
    assert_eq!(why(&settled(&mut s, sheet)), Some(None));
    let drawing = json!({"drawing": ids::encode(s.open_drawing.unwrap())});
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "joint.delete", json!({"joint": "Hinge"}));
    run(&mut s, "drawing.open", drawing);
    let answer = extract(&mut s, sheet).unwrap();
    assert!(answer.stale.is_empty(), "the picture is the same, so nothing draws again: {:?}", answer.stale);
    assert_eq!(why(&answer), Some(Some("its position names 1 joint the design no longer has".into())));
}

/// How far apart the tips of `group`'s two arrowheads are on paper.
fn tips_apart(group: &Group) -> f64 {
    let tips = text_and_tips(group).1;
    assert_eq!(tips.len(), 2, "{tips:?}");
    tips[0].dist(tips[1])
}

/// From the first cube's left corner to the second's right one in [`two_cubes`], horizontal: 40
/// assembled.
fn across_the_cubes(s: &Session) -> Value {
    let (first, second) = (s.doc.occurrences[0].id, s.doc.occurrences[1].id);
    json!({"linear": {"a": {"point": [0, 0, 0], "path": [first]}, "b": {"point": [40, 0, 0], "path": [second]}, "direction": "horizontal"}})
}

/// A dimension on a view that draws an exploded view is read and drawn where the view draws its parts
/// (#13048), not held back: across the cubes, 40 apart assembled, it is drawn 40 + 30 = 70 apart at
/// 1:1 once the view is exploded, and nothing lists it out of date: not the scene, the export or
/// `drawing.status`. Drawing the view from another exploded view, the second cube 60 out, draws it
/// again, 100 apart, and so does a deferred drawing once updated.
#[test]
fn a_dimension_on_an_exploded_view_is_drawn_where_the_view_draws_its_parts() {
    let (mut s, sheet, [assembled, ..], apart) = two_assembled_one_apart();
    let kind = across_the_cubes(&s);
    let id = id_in(run(&mut s, "drawing.dimension.add", json!({"view": ids::encode(assembled), "kind": kind})), "dimension");
    assert!((tips_apart(group_of(&built(&s, sheet).scene, id)) - 40.0).abs() < 1e-6);
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(assembled), "set": {"exploded": apart}}));
    let shown = settled(&mut s, sheet);
    assert!((tips_apart(group_of(&shown.scene, id)) - 70.0).abs() < 1e-6);
    assert_eq!(bytes(&shown.scene), bytes(&built(&s, sheet).scene));
    let scene = run(&mut s, "drawing.scene", json!({"sheet": ids::encode(sheet)}));
    assert!(scene.get("out_of_date").is_none(), "{scene}");
    let drawn: Vec<(Value, Value)> = scene["dimensions"].as_array().unwrap().iter().map(|d| (d["dimension"].clone(), d["drawn"].clone())).collect();
    assert_eq!(drawn, [(json!(ids::encode(id)), json!(true))], "{scene}");
    assert!(listed(&mut s, sheet).is_empty(), "nor does the export");
    let (reply, words) = status_of(&mut s);
    assert!(all_are(&words, "current") && out_of_date_dimensions(&reply).is_empty(), "{reply}");

    let drawing = json!({"drawing": ids::encode(s.open_drawing.unwrap())});
    run(&mut s, "drawing.close", json!({}));
    let second = s.doc.occurrences[1].id;
    run(&mut s, "explode.create", json!({"name": "Further", "moves": [{"occurrence": second, "translate": [60, 0, 0]}]}));
    run(&mut s, "drawing.open", drawing);
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(assembled), "set": {"exploded": "Further"}}));
    assert!((tips_apart(group_of(&settled(&mut s, sheet).scene, id)) - 100.0).abs() < 1e-6);

    // Deferred and updated, it is read where the view draws its parts too.
    run(&mut s, "drawing.set", json!({"updates": "deferred"}));
    run(&mut s, "drawing.update", json!({}));
    assert!((tips_apart(group_of(&settled(&mut s, sheet).scene, id)) - 100.0).abs() < 1e-6, "deferred");
    let scene = run(&mut s, "drawing.scene", json!({"sheet": ids::encode(sheet)}));
    assert!(scene.get("out_of_date").is_none(), "deferred: {scene}");
    assert!(out_of_date_dimensions(&status_of(&mut s).0).is_empty(), "deferred");
}

/// A note on the model of a view that draws an exploded view is read and drawn where the view draws
/// its parts, as a dimension is, whether its anchor or only a branch points at the model, and nothing
/// lists it out of date, deferred and updated too. Exploded, the view (at (60, 150), 1:1) is centred on the box's middle (35, 5),
/// so the second cube's bottom front edge's middle, (35 + 30, 0, 0), is at (60 + 30, 150 - 5).
#[test]
fn a_note_on_the_model_of_an_exploded_view_is_drawn_where_the_view_draws_its_parts() {
    let (mut s, sheet, [assembled, ..], apart) = two_assembled_one_apart();
    let second = s.doc.occurrences[1].id;
    let edge = json!({"point": [35, 0, 0], "path": [second]});
    let view = ids::encode(assembled);
    let note = |s: &mut Session, add: Value| id_in(run(s, "drawing.note.add", add), "note");
    let on_edge = note(&mut s, json!({"anchor": {"edge": {"view": view, "edge": edge}}, "at": [60, 40], "content": "Deburr"}));
    let branched = note(&mut s, json!({"anchor": {"view": view}, "at": [60, -40], "content": "Both", "branches": [{"edge": edge}]}));
    let plain = note(&mut s, json!({"anchor": {"view": view}, "at": [0, -40], "content": "Exploded"}));
    run(&mut s, "drawing.view.edit", json!({"view": view, "set": {"exploded": apart}}));
    let shown = settled(&mut s, sheet);
    for id in [on_edge, branched] {
        let tips = text_and_tips(group_of(&shown.scene, id)).1;
        assert!(tips.len() == 1 && tips[0].dist(Vec2::new(90.0, 145.0)) < 1e-6, "{id}: {tips:?}");
    }
    let mut notes = [on_edge, branched, plain];
    notes.sort_unstable();
    let drawn: Vec<u64> = shown.scene.groups.iter().filter(|g| g.kind == ObjectKind::Note).map(|g| g.object).collect();
    assert_eq!(drawn, notes);
    let scene = run(&mut s, "drawing.scene", json!({"sheet": ids::encode(sheet)}));
    assert!(scene.get("out_of_date").is_none(), "{scene}");
    assert!(listed(&mut s, sheet).is_empty(), "nor does the export");

    // Deferred and updated, they are read where the view draws its parts too.
    run(&mut s, "drawing.set", json!({"updates": "deferred"}));
    run(&mut s, "drawing.update", json!({}));
    let shown = settled(&mut s, sheet);
    for id in [on_edge, branched] {
        let tips = text_and_tips(group_of(&shown.scene, id)).1;
        assert!(tips.len() == 1 && tips[0].dist(Vec2::new(90.0, 145.0)) < 1e-6, "deferred {id}: {tips:?}");
    }
    let scene = run(&mut s, "drawing.scene", json!({"sheet": ids::encode(sheet)}));
    assert!(scene.get("out_of_date").is_none(), "deferred: {scene}");
}

/// A dimension on a view in a position of its own is read and drawn where the view draws its parts
/// (#13048), not held back: on [`hinge_pair`]'s view posed at 90°, from the plate's far top corner
/// (0, 0, 2) to the leaf's free top corner, (0, 40, 2) as designed, turned about the hinge line (y 20,
/// z 0) to y 20 + 20 cos 90° - 2 sin 90° = 18: 18 apart at 1:1. Posed at 30° instead, it is drawn
/// again: 20 + 20 cos 30° - 2 sin 30° = 36.3205 apart.
#[test]
fn a_dimension_on_a_posed_view_is_drawn_and_follows_its_position() {
    let (mut s, view, _) = hinge_pair();
    let sheet = s.drawing("test").unwrap().sheets[0].id;
    let leaf = s.doc.occurrences.iter().find(|o| o.name.starts_with("Leaf")).map(|o| o.id).unwrap();
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(view), "set": {"pose": [{"joint": "Hinge", "values": [90]}]}}));
    let kind = json!({"linear": {"a": [0, 0, 2], "b": {"point": [0, 40, 2], "path": [leaf]}, "direction": "vertical"}});
    let id = id_in(run(&mut s, "drawing.dimension.add", json!({"view": ids::encode(view), "kind": kind})), "dimension");
    let before = settled(&mut s, sheet);
    assert!((tips_apart(group_of(&before.scene, id)) - 18.0).abs() < 1e-6);
    assert!(run(&mut s, "drawing.scene", json!({"sheet": ids::encode(sheet)})).get("out_of_date").is_none());
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(view), "set": {"pose": [{"joint": "Hinge", "values": [30]}]}}));
    let after = settled(&mut s, sheet);
    let turned = 20.0 + 20.0 * 30f64.to_radians().cos() - 2.0 * 30f64.to_radians().sin();
    assert!((tips_apart(group_of(&after.scene, id)) - turned).abs() < 1e-6, "{turned}");
    assert_eq!(bytes(&after.scene), bytes(&built(&s, sheet).scene));
}

/// Each dimension on a sheet takes its own view's answer, whatever order its view's records come in
/// (#13048). [`hinge_pair`] with a pin and exploded view Apart moving the leaf 30 mm out along y, and
/// three top views at 1:1 whose ids put the exploded one first and the one of the design's placement
/// last, the other way round from the order their states sort in. One dimension on each, from the
/// plate's far top corner (0, 0, 2) to the leaf's free top corner (0, 40, 2): 40 + 30 = 70 exploded,
/// 20 - 2 = 18 posed at 90° (the corner turned up about the hinge line, y 20, z 0), 40 as designed.
#[test]
fn each_dimension_on_a_sheet_takes_its_own_views_answer() {
    use solidcraft_doc::drawing_view::{JointPose, Pose};
    let (mut s, _, hinge) = hinge_pair();
    let drawing = json!({"drawing": ids::encode(s.open_drawing.unwrap())});
    run(&mut s, "drawing.close", json!({}));
    add_a_pin(&mut s);
    let leaf = s.doc.occurrences.iter().find(|o| o.name.starts_with("Leaf")).map(|o| o.id).unwrap();
    let apart =
        run(&mut s, "explode.create", json!({"name": "Apart", "moves": [{"occurrence": leaf, "translate": [0, 30, 0]}]}))["view"].as_u64().unwrap();
    run(&mut s, "drawing.open", drawing);
    let mut d = (**s.drawing("test").unwrap()).clone();
    let (sheet, source) = (d.sheets[0].id, d.sources[0].id);
    let posed = Some(Pose::Joints(vec![JointPose { joint: hinge, values: vec![std::f64::consts::FRAC_PI_2] }].into()));
    // By id: exploded, posed, designed.
    let made = [(11, [80.0, 200.0], None, Some(Exploded::View(apart))), (12, [200.0, 200.0], posed, None), (13, [320.0, 200.0], None, None)];
    for (id, at, pose, exploded) in made {
        let settings = ViewOverrides { scale: Some("1:1".parse().unwrap()), pose, exploded, ..ViewOverrides::default() };
        let kind = ViewKind::Base { source, view: ViewDirection::Named(NamedView::Top) };
        d.add_views([Arc::new(View { id, sheet, kind, place: ViewPlace::At(at), settings, reference_shown_percent: 0.0, label: None })]);
    }
    d.validate().unwrap();
    s.doc_mut().put_drawing(Arc::new(d));
    let kind = json!({"linear": {"a": [0, 0, 2], "b": {"point": [0, 40, 2], "path": [leaf]}, "direction": "vertical"}});
    let on = |s: &mut Session, view: u64| id_in(run(s, "drawing.dimension.add", json!({"view": ids::encode(view), "kind": kind})), "dimension");
    let want = [(on(&mut s, 11), 70.0), (on(&mut s, 12), 18.0), (on(&mut s, 13), 40.0)];
    let built = built(&s, sheet);
    for (id, apart) in want {
        assert!((tips_apart(group_of(&built.scene, id)) - apart).abs() < 1e-6, "{id}: {}", tips_apart(group_of(&built.scene, id)));
    }
    assert_eq!(bytes(&settled(&mut s, sheet).scene), bytes(&built.scene));
}

/// A deferred drawing draws nothing until `drawing.update`: then from the model as it is now, until
/// that changes. Reopened, it waits again. While it waits, every command that reads its sheet reads
/// what the page shows and lists what is out of date, and `drawing.status` says it is held.
#[test]
fn a_deferred_drawing_keeps_its_lines_until_it_is_updated() {
    let (mut s, sheet, [front, top]) = front_and_top();
    let drawing = json!({"drawing": ids::encode(s.open_drawing.unwrap())});
    run(&mut s, "drawing.set", json!({"updates": "deferred"}));
    let checkpoint = run(&mut s, "edit.checkpoint", json!({}))["checkpoint"].clone();
    extract(&mut s, sheet).unwrap();
    assert!(!step(&mut s), "nothing is drawn before an update");
    assert!(begin(&mut s).is_none(), "nor asked for beside the window");
    assert_eq!(run(&mut s, "drawing.update", json!({}))["waiting"], 2);
    let before = settled(&mut s, sheet);
    assert!(before.stale.is_empty());

    run(&mut s, "drawing.close", json!({}));
    let block = s.doc.features[0].id;
    run(&mut s, "timeline.edit", json!({"feature": block.to_string(), "set": {"height": 40}}));
    run(&mut s, "drawing.open", drawing);
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(front), "set": {"scale": "1:4"}}));
    let held = settled(&mut s, sheet);
    assert_eq!((held.stale.len(), bytes(&held.scene)), (2, bytes(&before.scene)), "its last lines, out of date");
    let status = run(&mut s, "drawing.status", json!({}));
    assert_eq!((&status["held"], &status["waiting"]), (&json!(true), &json!(2)));
    let sheet_id = ids::encode(sheet);
    let scene = run(&mut s, "drawing.scene", json!({"sheet": sheet_id}));
    let mut listed: Vec<String> = serde_json::from_value(scene["out_of_date"].clone()).unwrap();
    listed.sort();
    let mut both = vec![ids::encode(front), ids::encode(top)];
    both.sort();
    assert_eq!(listed, both);
    assert_eq!(run(&mut s, "drawing.export", json!({"sheet": sheet_id}))["out_of_date"].as_array().map(Vec::len), Some(2));
    assert_eq!(s.drawing_scenes.projections, 0);

    run(&mut s, "drawing.update", json!({}));
    let now = settled(&mut s, sheet);
    assert_eq!((now.stale.len(), bytes(&now.scene)), (0, bytes(&built(&s, sheet).scene)));
    assert!(run(&mut s, "drawing.status", json!({})).get("held").is_none());
    assert!(run(&mut s, "drawing.scene", json!({"sheet": sheet_id})).get("out_of_date").is_none());

    // The next change waits again: back to 20 tall, its lines stay 40 tall.
    run(&mut s, "edit.restore_checkpoint", json!({"checkpoint": checkpoint}));
    let waiting = settled(&mut s, sheet);
    assert_eq!((waiting.stale.len(), bytes(&waiting.scene)), (2, bytes(&now.scene)));
}

/// The export scratch session shares the exact geometry of each drawn view and dimension with
/// the open session. A deep copy of the same geometry must not pass this pointer check (#4890).
#[test]
fn a_scratch_export_copy_shares_view_and_dimension_groups() {
    let (mut s, sheet, [front, top]) = front_and_top();
    let dimension = front_edge(&mut s, front);
    settled(&mut s, sheet);
    let scratch = s.scratch_as_shown();
    let same_group = |a: &Arc<Group>, b: &Arc<Group>| Arc::ptr_eq(a, b);

    for view in [front, top] {
        let original = s.drawing_scenes.views[&view].drawn.group.as_ref().expect("the view was drawn");
        let copied = scratch.drawing_scenes.views[&view].drawn.group.as_ref().expect("the scratch copy kept the view");
        assert!(same_group(original, copied), "view {view} must share the drawn group");
        let deep_clone = Arc::new((**original).clone());
        assert!(!same_group(original, &deep_clone), "a deep clone must fail the pointer check");
    }

    let original = s.drawing_scenes.dimensions[&dimension].drawn.group.as_ref().expect("the dimension was drawn");
    let copied = scratch.drawing_scenes.dimensions[&dimension].drawn.group.as_ref().expect("the scratch copy kept the dimension");
    assert!(same_group(original, copied), "the dimension must share its drawn group");
    let deep_clone = Arc::new((**original).clone());
    assert!(!same_group(original, &deep_clone), "a deep clone of a dimension must fail the pointer check");
}

/// The copy a background export runs on ([`Session::scratch_as_shown`]) exports a sheet as the
/// session does: a drawing that isn't deferred, drawn at once, also after a view's edit; a
/// deferred drawing after its update, with a view edited since; and one held since its update,
/// with its views' and dimensions' last lines, on the sheet last shown and on another.
#[test]
fn a_held_deferred_drawing_exports_the_same_from_a_scratch_copy() {
    let (mut s, sheet, [front, _]) = front_and_top();
    let svg = |sheet: u64| json!({"sheet": ids::encode(sheet), "format": "svg"});
    // The file and what it leaves out of date; a view drawn here again has its own timings.
    let shown = |reply: &Value| (reply["svg"].clone(), reply["out_of_date"].clone());
    let same = |s: &mut Session, sheet: u64| {
        let there = s.scratch_as_shown().execute("drawing.export", &svg(sheet)).unwrap();
        let here = run(s, "drawing.export", svg(sheet));
        assert_eq!(shown(&there), shown(&here));
        here
    };
    // The front view's bottom edge, so a dimension's last reading is held too.
    front_edge(&mut s, front);
    same(&mut s, sheet);
    // A view edited after the sheet was drawn: the copy draws it and its dimension again.
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(front), "set": {"scale": "1:4"}}));
    same(&mut s, sheet);
    // A second sheet, with a view of its own.
    run(&mut s, "drawing.sheet.add", json!({}));
    let mut d = (**s.drawing("test").unwrap()).clone();
    let (other, source) = (d.sheets[1].id, d.sources[0].id);
    let id = d.allocate_id(&mut solidcraft_doc::ids::IdSource::seeded(10431)).unwrap();
    let kind = ViewKind::Base { source, view: ViewDirection::Named(NamedView::Top) };
    let settings = ViewOverrides { scale: Some("1:2".parse().unwrap()), ..ViewOverrides::default() };
    let view = View { id, sheet: other, kind, place: ViewPlace::At([150.0, 150.0]), settings, reference_shown_percent: 0.0, label: None };
    d.add_views([Arc::new(view)]);
    d.validate().unwrap();
    s.doc_mut().put_drawing(Arc::new(d));

    let drawing = json!({"drawing": ids::encode(s.open_drawing.unwrap())});
    run(&mut s, "drawing.set", json!({"updates": "deferred"}));
    let checkpoint = run(&mut s, "edit.checkpoint", json!({}))["checkpoint"].clone();
    run(&mut s, "drawing.close", json!({}));
    let block = s.doc.features[0].id;
    run(&mut s, "timeline.edit", json!({"feature": block.to_string(), "set": {"height": 40}}));
    run(&mut s, "drawing.open", drawing);
    run(&mut s, "drawing.update", json!({}));
    settled(&mut s, other);
    settled(&mut s, sheet);
    // Updated, so not held: a view edited since draws at once, on the copy too, and its dimension
    // is read again.
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(front), "set": {"scale": "1:2"}}));
    let here = same(&mut s, sheet);
    assert_eq!(here["out_of_date"].as_array().map_or(0, Vec::len), 0, "drawn: {here}");
    // Held with the drawing open, every line drawn.
    run(&mut s, "edit.restore_checkpoint", json!({"checkpoint": checkpoint}));
    settled(&mut s, sheet);
    // Sheet 1: its two views and their dimension out of date, the dimension's last reading drawn; the
    // other sheet: its one view.
    for (on, listed, dimensions) in [(sheet, 3, 1), (other, 1, 0)] {
        let here = same(&mut s, on);
        let count = |key: &str| here[key].as_array().map(Vec::len);
        assert_eq!((count("out_of_date"), count("dimensions")), (Some(listed), Some(dimensions)), "held: {here}");
    }
}

/// The block's front bottom edge, 100 long, as a dimension on view `front`.
fn front_edge(s: &mut Session, front: u64) -> u64 {
    let added = run(s, "drawing.dimension.add", json!({"view": ids::encode(front), "kind": {"length": {"edge": [50, 0, 0]}}}));
    ids::decode(added["dimension"].as_str().unwrap()).unwrap()
}

/// `drawing.export`'s `out_of_date` for `sheet`, views first.
fn listed(s: &mut Session, sheet: u64) -> Vec<Value> {
    let reply = run(s, "drawing.export", json!({"sheet": ids::encode(sheet)}));
    reply["out_of_date"].as_array().cloned().unwrap_or_default()
}

/// A deferred drawing closed during a model change keeps its dimensions' last lines as it keeps its
/// views': opened again, held, it shows both as they were and lists both out of date. Updated, the
/// dimension is read again from the model as it is now. Held again with the drawing open, the
/// dimension is listed with its view.
#[test]
fn a_deferred_drawing_closed_during_a_change_keeps_its_dimensions_until_it_is_updated() {
    let (mut s, sheet, [front, top]) = front_and_top();
    let id = front_edge(&mut s, front);
    let drawing = json!({"drawing": ids::encode(s.open_drawing.unwrap())});
    run(&mut s, "drawing.set", json!({"updates": "deferred"}));
    run(&mut s, "drawing.update", json!({}));
    let before = settled(&mut s, sheet);
    let checkpoint = run(&mut s, "edit.checkpoint", json!({}))["checkpoint"].clone();
    let open = s.open_drawing.unwrap();
    run(&mut s, "drawing.close", json!({}));
    assert!(s.drawing_scenes.kept.of(open).is_some_and(|kept| kept.dimensions.contains_key(&id)), "kept at the close");
    let block = s.doc.features[0].id;
    run(&mut s, "timeline.edit", json!({"feature": block.to_string(), "set": {"length": 120}}));
    run(&mut s, "drawing.open", drawing);

    let held = settled(&mut s, sheet);
    assert_eq!(bytes(&held.scene), bytes(&before.scene), "its last views and dimension");
    let sorted = |ids: &[Value]| {
        let mut ids: Vec<String> = ids.iter().filter_map(|id| id.as_str().map(str::to_string)).collect();
        ids.sort();
        ids
    };
    let out = listed(&mut s, sheet);
    assert_eq!(out.len(), 3, "the views, then the dimension: {out:?}");
    assert_eq!((sorted(&out[..2]), &out[2]), (sorted(&[json!(ids::encode(front)), json!(ids::encode(top))]), &json!(ids::encode(id))));

    run(&mut s, "drawing.update", json!({}));
    let now = settled(&mut s, sheet);
    assert_eq!(bytes(&now.scene), bytes(&built(&s, sheet).scene), "read again: 120 long");
    assert_ne!(bytes_of(group_of(&now.scene, id)), bytes_of(group_of(&before.scene, id)));
    assert!(listed(&mut s, sheet).is_empty());

    run(&mut s, "edit.restore_checkpoint", json!({"checkpoint": checkpoint}));
    let out = listed(&mut s, sheet);
    assert_eq!(out.len(), 3, "the views and the dimension, held with the drawing open: {out:?}");
    assert_eq!(out[2], json!(ids::encode(id)));
}

/// A model change made with the drawing open (the model is no longer locked while one is) marks
/// its views out of date exactly as one made while it was closed: a deferred drawing keeps its last
/// lines and lists its views and their dimension out of date until Update reads the model again; an
/// automatic one lists every view out of date until the page draws them.
#[test]
fn a_model_change_with_the_drawing_open_marks_its_views_out_of_date() {
    let (mut s, sheet, [front, top]) = front_and_top();
    let id = front_edge(&mut s, front);
    let open = s.open_drawing;
    run(&mut s, "drawing.set", json!({"updates": "deferred"}));
    run(&mut s, "drawing.update", json!({}));
    let before = settled(&mut s, sheet);
    assert!(listed(&mut s, sheet).is_empty());
    let block = s.doc.features[0].id;
    run(&mut s, "timeline.edit", json!({"feature": block.to_string(), "set": {"length": 120}}));
    assert_eq!(s.open_drawing, open, "the drawing stays open");

    let held = settled(&mut s, sheet);
    assert_eq!(bytes(&held.scene), bytes(&before.scene), "its last views and dimension");
    let mut views: Vec<Value> = listed(&mut s, sheet);
    assert_eq!(views.len(), 3, "the views, then the dimension: {views:?}");
    assert_eq!(views.pop(), Some(json!(ids::encode(id))));
    views.sort_by_key(|v| v.as_str().map(str::to_string));
    let mut expected = vec![json!(ids::encode(front)), json!(ids::encode(top))];
    expected.sort_by_key(|v| v.as_str().map(str::to_string));
    assert_eq!(views, expected);

    run(&mut s, "drawing.update", json!({}));
    let now = settled(&mut s, sheet);
    assert_eq!(bytes(&now.scene), bytes(&built(&s, sheet).scene), "read again: 120 long");
    assert!(listed(&mut s, sheet).is_empty());

    run(&mut s, "drawing.set", json!({"updates": "automatic"}));
    settled(&mut s, sheet);
    assert!(all_are(&status_of(&mut s).1, "current"));
    run(&mut s, "timeline.edit", json!({"feature": block.to_string(), "set": {"length": 90}}));
    assert!(all_are(&status_of(&mut s).1, "out_of_date"), "a model edit with the drawing open marks every view");
}

/// A dimension gone while its drawing was closed (a checkpoint from before it was added, restored) is
/// not shown, listed or kept when the drawing opens again, held.
#[test]
fn a_dimension_gone_while_its_drawing_was_closed_is_not_shown_or_kept() {
    let (mut s, sheet, [front, _]) = front_and_top();
    let drawing = json!({"drawing": ids::encode(s.open_drawing.unwrap())});
    run(&mut s, "drawing.set", json!({"updates": "deferred"}));
    run(&mut s, "drawing.update", json!({}));
    let without = settled(&mut s, sheet);
    let checkpoint = run(&mut s, "edit.checkpoint", json!({}))["checkpoint"].clone();
    let id = front_edge(&mut s, front);
    settled(&mut s, sheet);
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "edit.restore_checkpoint", json!({"checkpoint": checkpoint}));
    run(&mut s, "drawing.open", drawing);
    let reopened = settled(&mut s, sheet);
    assert_eq!(bytes(&reopened.scene), bytes(&without.scene));
    assert!(!listed(&mut s, sheet).contains(&json!(ids::encode(id))));
    assert!(!s.drawing_scenes.dimensions.contains_key(&id));
}

/// A held deferred drawing closed, its model changed and opened again exports from the copy a
/// background export runs on as the session does, before anything reads its sheet: its views' and
/// its dimension's kept lines, out of date. The copy goes first, as a file export does, since the
/// session's own export takes the kept lines back.
#[test]
fn a_reopened_held_drawing_exports_the_same_from_a_scratch_copy_before_its_sheet_is_read() {
    let (mut s, sheet, [front, _]) = front_and_top();
    front_edge(&mut s, front);
    let drawing = json!({"drawing": ids::encode(s.open_drawing.unwrap())});
    run(&mut s, "drawing.set", json!({"updates": "deferred"}));
    run(&mut s, "drawing.update", json!({}));
    settled(&mut s, sheet);
    run(&mut s, "drawing.close", json!({}));
    let block = s.doc.features[0].id;
    run(&mut s, "timeline.edit", json!({"feature": block.to_string(), "set": {"height": 40}}));
    run(&mut s, "drawing.open", drawing);
    let svg = json!({"sheet": ids::encode(sheet), "format": "svg"});
    let there = s.scratch_as_shown().execute("drawing.export", &svg).unwrap();
    let here = run(&mut s, "drawing.export", svg);
    assert_eq!(here["out_of_date"].as_array().map(Vec::len), Some(3), "held, its two views' and its dimension's last lines: {here}");
    assert_eq!((&there["svg"], &there["out_of_date"]), (&here["svg"], &here["out_of_date"]));
}

/// `drawing.update` on a drawing that follows the model changes nothing, and draws nothing itself.
#[test]
fn updating_an_automatic_drawing_changes_nothing() {
    let (mut s, sheet, _) = front_and_top();
    let doc = s.doc.clone();
    assert_eq!(run(&mut s, "drawing.update", json!({}))["waiting"], 2);
    assert_eq!(s.drawing_scenes.projections, 0);
    assert!(Arc::ptr_eq(&doc, &s.doc));
    assert!(settled(&mut s, sheet).stale.is_empty());
}

/// Opened again after a model change, with a view's scale changed too, a drawing shows its last
/// lines, out of date, until each view is drawn; then it draws as the built scene does.
#[test]
fn a_drawing_opened_after_a_change_shows_its_last_lines_until_drawn() {
    let (mut s, sheet, [front, top]) = front_and_top();
    let drawing = json!({"drawing": ids::encode(s.open_drawing.unwrap())});
    let before = settled(&mut s, sheet);
    run(&mut s, "drawing.close", json!({}));
    let block = s.doc.features[0].id;
    run(&mut s, "timeline.edit", json!({"feature": block.to_string(), "set": {"height": 40}}));
    run(&mut s, "drawing.open", drawing);
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(front), "set": {"scale": "1:4"}}));
    let reopened = extract(&mut s, sheet).unwrap();
    assert_eq!(reopened.stale.len(), 2);
    for id in [front, top] {
        assert_eq!(bytes_of(group_of(&reopened.scene, id)), bytes_of(group_of(&before.scene, id)), "{id}");
    }
    let now = settled(&mut s, sheet);
    assert_eq!((bytes(&now.scene), s.drawing_scenes.projections), (bytes(&built(&s, sheet).scene), 2));
}

/// A drawing's kept lines go with it: deleted, nothing of it is kept, views or dimensions.
#[test]
fn a_deleted_drawings_lines_are_not_kept() {
    let (mut s, sheet, [front, _]) = front_and_top();
    let drawing = s.open_drawing.unwrap();
    front_edge(&mut s, front);
    settled(&mut s, sheet);
    run(&mut s, "drawing.close", json!({}));
    let kept = |s: &Session| s.drawing_scenes.kept.of(drawing).map(|kept| (kept.views.len(), kept.dimensions.len()));
    assert_eq!(kept(&s), Some((2, 1)));
    run(&mut s, "drawing.delete", json!({"drawing": ids::encode(drawing)}));
    assert_eq!(kept(&s), None);
}

/// A set with nothing to show has no key, so a view drawn from it always draws again.
#[test]
fn nothing_to_show_has_no_source_key() {
    assert_eq!(SourceKey::of(&Err(Unmeasured::NoBodies)), None);
}

/// A view whose lines would take the sheet past a paper scene's records fails and is left out;
/// a view after it that fits is still drawn, up to the limit exactly. A title block's records are
/// kept for it, though it is drawn after the views.
#[test]
fn a_view_past_the_sheets_records_leaves_out_only_itself() {
    let (s, sheet, _) = front_and_top();
    let d = s.drawing("test").unwrap().clone();
    // One stroke of `n` pieces: n + 2 records with its group.
    let lines = |n: usize| {
        let path = (0..n).map(|i| Seg2::Line { a: Vec2::new(i as f64 * 1e-3, 0.0), b: Vec2::new((i + 1) as f64 * 1e-3, 0.0) }).collect();
        let items = vec![Item::Stroke { path, closed: false, width_mm: 0.5, pattern: None }];
        Drawn {
            hatches: Vec::new(),
            group: Some(Arc::new(Group { object: n as u64, kind: ObjectKind::View, items })),
            status: Status::Drawn,
            stats: None,
            boundary: None,
            frame: None,
            owners: None,
            why: None,
        }
    };
    for title_block in [None, Some(solidcraft_geom::title_block::default_block())] {
        let record = Sheet { title_block, ..(**d.sheets.iter().find(|x| x.id == sheet).unwrap()).clone() };
        let (empty, ..) = assemble(&record, Projection::Third, &labels(&record), &[], &[], &[], std::iter::empty::<(u64, Vec2, Drawn)>())
            .unwrap()
            .finish()
            .unwrap();
        assert_eq!(empty.groups.len(), if record.title_block.is_some() { 4 } else { 3 });
        // The block's text and its fonts count too.
        let fonts = |scene: &Scene| Scene::new(scene.size_mm, Vec::new()).font_records_beyond(&scene.fonts);
        let left = MAX_RECORDS - empty.groups.iter().map(Group::records).sum::<usize>() - fonts(&empty);
        // The first leaves 20 records; the second needs 32; the third takes the last 20.
        let views = [lines(left - 22), lines(30), lines(18)];
        let centre = Vec2::new(100.0, 100.0);
        let views = views.iter().enumerate().map(|(i, v)| (i as u64, centre, v));
        let (scene, states, ..) = assemble(&record, Projection::Third, &labels(&record), &[], &[], &[], views).unwrap().finish().unwrap();
        let statuses: Vec<&'static str> = states.iter().map(|v| v.status.name()).collect();
        assert_eq!(statuses, ["drawn", "failed", "drawn"]);
        let records = scene.groups.iter().map(Group::records).sum::<usize>() + fonts(&scene);
        assert_eq!((scene.groups.len(), records), (empty.groups.len() + 2, MAX_RECORDS));
        assert!(matches!(&states[1].status, Status::Failed(why) if why.contains("put it on another sheet")), "{:?}", states[1]);
    }
}

/// A view at 200:1 asks the mesh for chords no finer than its floor, so it is drawn rather than
/// refused for a chord it has no setting for.
#[test]
fn a_view_past_100_to_1_is_drawn_with_a_coarser_chord() {
    let (mut s, sheet, [front, _]) = front_and_top();
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(front), "set": {"scale": "200:1"}}));
    let scene = built(&s, sheet);
    let front = scene.views.iter().find(|v| v.view == front).unwrap();
    assert_eq!(front.status, Status::Drawn, "{front:?}");
}

/// Seeded mixes of moves, setting changes, undos, redos, extracts and steps: once nothing is
/// waiting, the provider's scene is always the built one, byte for byte.
#[test]
fn seeded_edits_and_steps_always_end_at_the_built_scene() {
    let pick = |seed: u64, salt: u64, n: u64| ((seed ^ salt).wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 33) % n;
    for seed in 1..=24u64 {
        let (mut s, sheet, [front, top]) = front_and_top();
        let dimension = top_edge(&mut s, front);
        let edit =
            |s: &mut Session, view: u64, set: Value| s.execute("drawing.view.edit", &json!({"view": ids::encode(view), "set": set})).map(|_| ());
        for i in 0..12u64 {
            let along = 100.0 + 10.0 * pick(seed, 100 + i, 10) as f64;
            let scale = ["1:1", "1:2", "1:5"][pick(seed, 200 + i, 3) as usize];
            let tangent_edges = ["hide", "show"][pick(seed, 300 + i, 2) as usize];
            // Refused edits (nothing to undo, say) change nothing, which the end still checks.
            let _ = match pick(seed, i, 10) {
                0 => edit(&mut s, front, json!({"place": {"at": [along, 150.0]}})),
                1 => edit(&mut s, top, json!({"place": {"aligned": along + 100.0}})),
                2 => edit(&mut s, front, json!({"scale": scale})),
                3 => edit(&mut s, top, json!({"tangent_edges": tangent_edges})),
                4 => s.execute("edit.undo", &json!({})).map(|_| ()),
                5 => s.execute("edit.redo", &json!({})).map(|_| ()),
                6 => {
                    let source = [json!({"occurrences": [[]]}), json!(null)][pick(seed, 400 + i, 2) as usize].clone();
                    edit(&mut s, top, json!({"source": source}))
                }
                7 => {
                    step(&mut s);
                    Ok(())
                }
                8 => {
                    s.execute("drawing.dimension.edit", &json!({"dimension": ids::encode(dimension), "set": {"offset_mm": along / 10.0}})).map(|_| ())
                }
                _ => extract(&mut s, sheet).map(drop),
            };
        }
        let end = settled(&mut s, sheet);
        assert!(end.stale.is_empty(), "seed {seed}");
        let at_once = built(&s, sheet);
        assert_eq!(bytes(&end.scene), bytes(&at_once.scene), "seed {seed}");
        assert_eq!(&*end.objects, at_once.objects.as_slice(), "seed {seed}");
        assert_eq!(end.dimensions, at_once.dimensions, "seed {seed}");
        assert_eq!(run(&mut s, "drawing.status", json!({}))["waiting"], json!(0), "seed {seed}");
    }
}

/// Two parts, each a 20 x 20 x 10 box: `Under` at the origin and `Over` moved 10 along X and 10 up, so
/// they span x 0..30 and z 0..20. The design and `Over`'s occurrence.
fn under_and_over() -> (Session, u64) {
    let mut design = Session::default();
    let mut part = |name: &str, body: &str| {
        let occurrence = run(&mut design, "component.create", json!({"name": name}))["occurrence"].as_u64().unwrap();
        run(&mut design, "solid.box", json!({"length": 20, "width": 20, "height": 10, "body_name": body, "operation": "new"}));
        run(&mut design, "component.activate", json!({"component": 0}));
        occurrence
    };
    part("Under", "BodyUnder");
    let over = part("Over", "BodyOver");
    run(&mut design, "occurrence.move", json!({"occurrence": over, "translate": [10, 0, 10]}));
    (design, over)
}

/// Setting a part as reference evaluates nothing, yet the views of it are out of date: its lines
/// join the reference parts', so no longer hide what is under them or stop at its own extents.
#[test]
fn a_change_of_reference_parts_redraws_the_views() {
    let (design, over) = under_and_over();
    let (mut s, sheet, [front, top]) = front_and_top_of(design);
    let before = settled(&mut s, sheet);
    let drawing = json!({"drawing": ids::encode(s.drawing("test").unwrap().id)});
    let in_the_model = |s: &mut Session, id: &str, p: Value| {
        run(s, "drawing.close", json!({}));
        run(s, id, p);
        run(s, "drawing.open", drawing.clone());
    };
    in_the_model(&mut s, "occurrence.bom_type", json!({"occurrence": over, "bom_type": "reference"}));
    let stale = extract(&mut s, sheet).unwrap().stale;
    assert!(stale.len() == 2 && stale.contains(&front) && stale.contains(&top), "{stale:?}");
    let after = settled(&mut s, sheet);
    assert_ne!(bytes(&before.scene), bytes(&after.scene));
    let thin = |scene: &Scene| {
        let items = scene.groups.iter().filter(|g| g.kind == ObjectKind::View).flat_map(|g| &g.items);
        items.filter(|i| matches!(i, Item::Stroke { width_mm, .. } if *width_mm == LineKind::Thin.width_mm(LineGroup::DEFAULT))).count()
    };
    assert_eq!(thin(&before.scene), 0);
    assert!(thin(&after.scene) > 0);
    let scene = built(&s, sheet);
    assert_eq!(bytes(&scene.scene), bytes(&after.scene), "the provider and a build draw one thing");
    // Undone, the views draw again, as they were; redone, as they were then.
    in_the_model(&mut s, "edit.undo", json!({}));
    assert_eq!(extract(&mut s, sheet).unwrap().stale.len(), 2);
    assert_eq!(bytes(&settled(&mut s, sheet).scene), bytes(&before.scene));
    in_the_model(&mut s, "edit.redo", json!({}));
    assert_eq!(extract(&mut s, sheet).unwrap().stale.len(), 2);
    assert_eq!(bytes(&settled(&mut s, sheet).scene), bytes(&after.scene));
    // Set on its component, it draws the same.
    in_the_model(&mut s, "occurrence.bom_type", json!({"occurrence": over, "bom_type": null}));
    assert_eq!(bytes(&settled(&mut s, sheet).scene), bytes(&before.scene));
    in_the_model(&mut s, "component.metadata.set", json!({"component": "Over", "bom_type": "reference"}));
    assert_eq!(extract(&mut s, sheet).unwrap().stale.len(), 2);
    assert_eq!(bytes(&settled(&mut s, sheet).scene), bytes(&after.scene));
    // A saved view takes the setting: the reference part now covers what is under it.
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(top), "set": {"reference_hides_model": true}}));
    assert_ne!(bytes(&settled(&mut s, sheet).scene), bytes(&after.scene));
}

/// A sheet's line group draws its views and dimensions again in that group's widths: at 0.7 a
/// visible edge is 0.7 wide, a reference part's edge 0.35, its hidden edges 0.35 dashed 12 and 3
/// widths, a dimension's lines 0.35, also when it alone is drawn again. `build` and the provider
/// draw one thing, and the 0.5 group chosen again, or followed again, gives back today's bytes.
#[test]
fn a_change_of_line_group_redraws_the_views_in_its_widths() {
    let mut design = Session::default();
    let mut part = |name: &str, body: &str| {
        let occurrence = run(&mut design, "component.create", json!({"name": name}))["occurrence"].as_u64().unwrap();
        run(&mut design, "solid.box", json!({"length": 20, "width": 20, "height": 10, "body_name": body, "operation": "new"}));
        run(&mut design, "component.activate", json!({"component": 0}));
        occurrence
    };
    part("Front", "BodyFront");
    let behind = part("Behind", "BodyBehind");
    run(&mut design, "occurrence.move", json!({"occurrence": behind, "translate": [10, 10, 0]}));
    run(&mut design, "occurrence.bom_type", json!({"occurrence": behind, "bom_type": "reference"}));
    let (mut s, sheet, [front, top]) = front_and_top_of(design);
    let shown_whole = json!({"reference_style": "visible_hidden", "reference_shown_percent": 100});
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(front), "set": shown_whole}));
    let added = run(&mut s, "drawing.dimension.add", json!({"view": ids::encode(front), "kind": {"linear": {"a": [0, 0, 10], "b": [20, 0, 10]}}}));
    let dimension = ids::decode(added["dimension"].as_str().unwrap()).unwrap();
    let before = settled(&mut s, sheet);
    let set = |s: &mut Session, group: Value| run(s, "drawing.sheet.set", json!({"sheet": ids::encode(sheet), "line_group": group}));
    set(&mut s, json!("0.7"));
    let stale = extract(&mut s, sheet).unwrap().stale;
    assert!(stale.len() == 2 && stale.contains(&front) && stale.contains(&top), "{stale:?}");
    let after = settled(&mut s, sheet);
    let strokes = |group: &Group| {
        let mut found: Vec<(f64, Option<Vec<f64>>)> = group
            .items
            .iter()
            .filter_map(|item| match item {
                Item::Stroke { width_mm, pattern, .. } => Some((*width_mm, pattern.as_ref().map(|p| p.lengths_mm.clone()))),
                _ => None,
            })
            .collect();
        found.sort_by(|a, b| a.partial_cmp(b).unwrap());
        found.dedup();
        found
    };
    let group = LineGroup::Mm0_7;
    let (wide, narrow) = (LineKind::Visible.width_mm(group), LineKind::Thin.width_mm(group));
    let dashes = LineKind::Hidden.dashes_mm(group).unwrap().to_vec();
    assert_eq!((wide, narrow, LineKind::Hidden.width_mm(group)), (0.7, 0.35, 0.35));
    assert_eq!(strokes(group_of(&after.scene, front)), [(narrow, None), (narrow, Some(dashes)), (wide, None)]);
    assert_eq!(strokes(group_of(&after.scene, dimension)), [(narrow, None)]);
    assert_eq!(bytes(&built(&s, sheet).scene), bytes(&after.scene), "the provider and a build draw one thing");
    // A dimension drawn again on its own keeps its view's group.
    run(&mut s, "drawing.dimension.edit", json!({"dimension": ids::encode(dimension), "set": {"offset_mm": 15.0}}));
    assert_eq!(strokes(group_of(&settled(&mut s, sheet).scene, dimension)), [(narrow, None)]);
    run(&mut s, "edit.undo", json!({}));
    set(&mut s, json!("0.5"));
    assert_eq!(bytes(&settled(&mut s, sheet).scene), bytes(&before.scene));
    set(&mut s, Value::Null);
    assert_eq!(bytes(&settled(&mut s, sheet).scene), bytes(&before.scene));
}

/// A sheet's reply with text keeps to its schema: its runs, the font they name, and never the font's
/// bytes.
#[test]
fn a_replys_text_and_fonts_keep_to_its_schema() {
    let style = solidcraft_sketch::TextStyle::default();
    let (runs, fonts) = solidcraft_sketch::paper_text("\u{d8}12", None, "", Vec2::new(20.0, 30.0), 3.5, 0.5, &style).unwrap().unwrap();
    let items = runs.into_iter().map(Item::Text).collect();
    let mut scene = Scene::new(Vec2::new(420.0, 297.0), vec![Group { object: 1, kind: ObjectKind::View, items }]);
    scene.add_fonts(fonts);
    scene.validate().unwrap();
    let reply = SheetSceneResult {
        sheet: "s".into(),
        scene: SheetPaperScene(scene),
        views: Vec::new(),
        objects: Vec::new(),
        dimensions: Vec::new(),
        sketches: Vec::new(),
        fills: Vec::new(),
        notes: Vec::new(),
        balloons: Vec::new(),
        tables: Vec::new(),
        tags: Vec::new(),
        out_of_date: Vec::new(),
        param_warnings: Vec::new(),
    };
    let reply = serde_json::to_value(reply).unwrap();
    let schema = output_schema_for::<SheetSceneResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &reply), "{reply}");
    assert_eq!(reply["scene"]["groups"][0]["items"][0]["kind"], json!("text"));
    assert_eq!(reply["scene"]["fonts"][0]["id"], reply["scene"]["groups"][0]["items"][0]["font"]);
    assert!(reply["scene"]["fonts"][0].get("bytes").is_none());
}

/// A run of text moves with its group: its start moves, and nothing else changes.
#[test]
fn text_moves_with_its_group() {
    let run = TextRun {
        font: [0; 32],
        scale: 0.005,
        origin: Vec2::new(1.0, 2.0),
        angle: 0.5,
        slant: 0.2,
        glyphs: vec![],
        text: String::new(),
        clusters: vec![],
        rtl: false,
    };
    let group = Group { object: 1, kind: ObjectKind::View, items: vec![Item::Text(run.clone())] };
    let Item::Text(placed) = &moved(&group, Vec2::new(10.0, 20.0)).items[0] else { panic!("a run") };
    assert_eq!(placed, &TextRun { origin: Vec2::new(11.0, 22.0), ..run });
}

/// A saved view's reference_shown_percent redraws that view alone, grows its boundary on the reference
/// part's side, leaves the model's strokes where they were, and goes back with an undo.
#[test]
fn a_saved_views_reference_shown_percent_grows_only_what_it_should() {
    let (mut design, over) = under_and_over();
    run(&mut design, "occurrence.bom_type", json!({"occurrence": over, "bom_type": "reference"}));
    let (mut s, sheet, [_, top]) = front_and_top_of(design);
    let strokes = |scene: &Scene, reference: bool| {
        let items = scene.groups.iter().filter(|g| g.kind == ObjectKind::View).flat_map(|g| &g.items);
        let thin = |i: &&Item| matches!(i, Item::Stroke { width_mm, .. } if *width_mm == LineKind::Thin.width_mm(LineGroup::DEFAULT));
        let mut drawn: Vec<String> = items.filter(|i| matches!(i, Item::Stroke { .. }) && thin(i) == reference).map(|i| format!("{i:?}")).collect();
        drawn.sort();
        drawn
    };
    let before = settled(&mut s, sheet);
    let boundary = |s: &mut Session| run(s, "drawing.view.inspect", json!({"view": ids::encode(top)}))["boundary_mm"].clone();
    let (min, max) = {
        let b = boundary(&mut s);
        (b["min"].clone(), b["max"].clone())
    };
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(top), "set": {"reference_shown_percent": 100}}));
    assert_eq!(extract(&mut s, sheet).unwrap().stale, [top]);
    let after = settled(&mut s, sheet);
    assert_eq!(strokes(&before.scene, false), strokes(&after.scene, false), "the model stays where it is");
    assert_ne!(strokes(&before.scene, true), strokes(&after.scene, true));
    let grown = boundary(&mut s);
    assert_eq!(grown["min"], min, "the side it doesn't pass stays");
    assert_ne!(grown["max"], max);
    assert_eq!(bytes(&built(&s, sheet).scene), bytes(&after.scene), "the provider and a build draw one thing");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(bytes(&settled(&mut s, sheet).scene), bytes(&before.scene));
}

fn near(b: &Aabb2, min: [f64; 2], max: [f64; 2]) -> bool {
    (b.min - Vec2::from(min)).len() < 1e-9 && (b.max - Vec2::from(max)).len() < 1e-9
}

/// A sheet's objects are its border, once, then each view by id. At 2:1 the block's front view is
/// 200 x 40 mm round (150, 150) and its top view 200 x 100 mm round (150, 220); each draws up to its
/// boundary, and inspect says the same boundary. The command answers them beside the scene, not in
/// it, as its schema says.
#[test]
fn a_sheets_objects_say_where_each_one_is() {
    let (mut s, sheet, [front, top]) = front_and_top();
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(front), "set": {"scale": "2:1"}}));
    let objects = built(&s, sheet).objects;
    let named: Vec<(u64, ObjectKind)> = objects.iter().map(|o| (o.object, o.kind)).collect();
    let by_id = [front.min(top), front.max(top)];
    assert_eq!(named, [(sheet, ObjectKind::DrawingBorder), (by_id[0], ObjectKind::View), (by_id[1], ObjectKind::View)]);
    let border = objects[0];
    assert_eq!(border.boundary_mm, None);
    assert!(
        border.bounds_mm.min.x >= 0.0 && border.bounds_mm.min.y >= 0.0 && border.bounds_mm.max.x <= 420.0 && border.bounds_mm.max.y <= 297.0,
        "{border:?}"
    );
    for (id, min, max) in [(front, [50.0, 130.0], [250.0, 170.0]), (top, [50.0, 170.0], [250.0, 270.0])] {
        let object = objects[if id == by_id[0] { 1 } else { 2 }];
        assert!(near(&object.bounds_mm, min, max), "{object:?}");
        assert!(near(&object.boundary_mm.unwrap(), min, max), "{object:?}");
        let inspected = run(&mut s, "drawing.view.inspect", json!({"view": ids::encode(id)}));
        let corner = |at: &str| Vec2::new(inspected["boundary_mm"][at][0].as_f64().unwrap(), inspected["boundary_mm"][at][1].as_f64().unwrap());
        assert!(
            (corner("min") - object.boundary_mm.unwrap().min).len() < 1e-9 && (corner("max") - object.boundary_mm.unwrap().max).len() < 1e-9,
            "{inspected}"
        );
    }
    let reply = run(&mut s, CMD, json!({"sheet": ids::encode(sheet)}));
    let schema = output_schema_for::<SheetSceneResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &reply), "{reply}");
    assert_eq!((&reply["objects"][1]["object"], &reply["objects"][1]["kind"]), (&json!(ids::encode(by_id[0])), &json!("view")), "{reply}");
    assert_eq!(reply["objects"][0].get("boundary_mm"), None, "{reply}");
    assert_eq!(reply["scene"].get("objects"), None, "{reply}");
}

/// A view that can't be drawn can still be picked: a failed one by its boundary, and a placeholder by
/// the 20 x 20 mm round its centre.
#[test]
fn a_view_with_no_lines_is_an_object_at_its_boundary() {
    let (mut s, sheet, [front, top]) = front_and_top();
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(top), "set": {"visual_style": "shaded"}}));
    let object = |s: &Session, id: u64| built(s, sheet).objects.into_iter().find(|o| o.object == id).unwrap();
    let failed = object(&s, top);
    assert!(near(&failed.bounds_mm, [125.0, 207.5], [175.0, 232.5]) && failed.boundary_mm == Some(failed.bounds_mm), "{failed:?}");
    for level in 0..=MAX_PLACEMENT_DEPTH as u64 + 1 {
        nest(&mut s, level);
    }
    let held = object(&s, front);
    assert!(near(&held.bounds_mm, [140.0, 140.0], [160.0, 160.0]) && held.boundary_mm == Some(held.bounds_mm), "{held:?}");
}

/// What draws nothing on paper is no object: a note on the sheet whose one field has no value is in
/// the scene's notes, drawn, but not among its objects, so nothing picks it and `drawing.scene` never
/// answers a box with no corners. The note beside it that prints a word is an object.
#[test]
fn a_note_that_prints_nothing_is_no_object() {
    let (mut s, sheet, _) = front_and_top();
    let note = |s: &mut Session, at: [f64; 2], content: Value| {
        let added = run(s, "drawing.note.add", json!({"anchor": {"sheet": ids::encode(sheet)}, "at": at, "content": content}));
        ids::decode(added["note"].as_str().unwrap()).unwrap()
    };
    let empty = note(&mut s, [20.0, 20.0], json!([{"field": {"source": "component", "key": "part_number"}}]));
    let printed = note(&mut s, [20.0, 40.0], json!("General"));
    let reply = run(&mut s, CMD, json!({"sheet": ids::encode(sheet)}));
    let schema = output_schema_for::<SheetSceneResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &reply), "{}", reply["objects"]);
    let shown = built(&s, sheet);
    let notes: Vec<u64> = shown.objects.iter().filter(|o| o.kind == ObjectKind::Note).map(|o| o.object).collect();
    assert_eq!(notes, [printed], "{:?}", shown.objects);
    assert!(shown.notes.iter().any(|n| n.note == empty && n.drawn), "{:?}", shown.notes);
}

/// A view's box holds what it draws, whatever that is, and one that drew nothing stands at its
/// boundary rather than at an empty box.
#[test]
fn a_views_box_is_what_it_draws() {
    let (s, ..) = front_and_top();
    let record = s.drawing("test").unwrap().sheets[0].clone();
    let (centre, boundary) = (Vec2::new(100.0, 100.0), Aabb2 { min: Vec2::new(-30.0, -20.0), max: Vec2::new(30.0, 20.0) });
    let rect = Aabb2 { min: Vec2::new(1.0, 2.0), max: Vec2::new(5.0, 7.0) };
    let corners = [rect.min, Vec2::new(rect.max.x, rect.min.y), rect.max, Vec2::new(rect.min.x, rect.max.y)];
    let segs = (0..4).map(|i| Seg2::Line { a: corners[i], b: corners[(i + 1) % 4] }).collect();
    let fill = Item::Fill { region: Region2 { outer: Loop2 { segs }, holes: Vec::new() } };
    let picture = Item::Picture(Picture { source: 1, rect, opacity: 1.0 });
    let moved = [101.0, 102.0];
    let moved_to = [105.0, 107.0];
    for (items, min, max) in [(vec![], [70.0, 80.0], [130.0, 120.0]), (vec![fill], moved, moved_to), (vec![picture], moved, moved_to)] {
        let drawn = Drawn {
            hatches: Vec::new(),
            group: Some(Arc::new(Group { object: 7, kind: ObjectKind::View, items })),
            status: Status::Drawn,
            stats: None,
            boundary: Some(boundary),
            frame: None,
            owners: None,
            why: None,
        };
        let (_, _, objects, _, _, _) =
            assemble(&record, Projection::Third, &labels(&record), &[], &[], &[], [(7, centre, &drawn)].into_iter()).unwrap().finish().unwrap();
        let object = objects.iter().find(|o| o.object == 7).unwrap();
        assert!(near(&object.bounds_mm, min, max), "{object:?}");
    }
}

/// The provider lists a view once it is drawn, moves its bounds with it and projects nothing for
/// that, and a view gone from the drawing is gone from the list.
#[test]
fn the_providers_objects_follow_its_views() {
    let (mut s, sheet, [front, top]) = front_and_top();
    let first = extract(&mut s, sheet).unwrap();
    assert_eq!(first.objects.iter().map(|o| o.object).collect::<Vec<_>>(), [sheet], "only what is drawn so far");
    let drawn = settled(&mut s, sheet);
    let at = |view: &SheetView, id: u64| view.objects.iter().find(|o| o.object == id).copied().unwrap();
    let before = at(&drawn, front);
    let projections = s.drawing_scenes.projections;
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(front), "set": {"place": {"at": [160.0, 150.0]}}}));
    let moved = settled(&mut s, sheet);
    assert_eq!(s.drawing_scenes.projections, projections, "a move draws nothing");
    let by = Vec2::new(10.0, 0.0);
    assert!(near(&at(&moved, front).bounds_mm, (before.bounds_mm.min + by).into(), (before.bounds_mm.max + by).into()), "{moved:?}");
    assert!(
        near(&at(&moved, top).bounds_mm, (at(&drawn, top).bounds_mm.min + by).into(), (at(&drawn, top).bounds_mm.max + by).into()),
        "the view lined up with it goes along"
    );
    assert_eq!(&*moved.objects, built(&s, sheet).objects.as_slice());
}

/// `drawing.status`, matching its schema, and each view's status word by id.
fn status_of(s: &mut Session) -> (Value, BTreeMap<u64, String>) {
    let reply = run(s, "drawing.status", json!({}));
    let schema = output_schema_for::<crate::schema::drawing_scene::DrawingStatusResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &reply), "{reply}");
    let words = reply["views"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| (ids::decode(v["view"].as_str().unwrap()).unwrap(), v["status"].as_str().unwrap().to_string()));
    let words = words.collect();
    (reply, words)
}

/// Every word of `words` is `word`.
fn all_are(words: &BTreeMap<u64, String>, word: &str) -> bool {
    !words.is_empty() && words.values().all(|w| w == word)
}

/// A view never drawn is out of date; `step` draws it and it is current. A model edit marks every
/// view; a move marks none, and neither does its undo; a change of a setting marks the view and
/// those following it. Reading draws nothing and changes nothing.
#[test]
fn status_says_which_views_are_out_of_date() {
    let (mut s, sheet, [front, top]) = front_and_top();
    let (reply, words) = status_of(&mut s);
    assert!(all_are(&words, "out_of_date"), "{reply}");
    assert_eq!(reply["waiting"], json!(2), "{reply}");
    extract(&mut s, sheet).unwrap();
    step(&mut s);
    // A step draws the lowest id first, and the seeded ids needn't put the front view there.
    let (first, second) = if front < top { (front, top) } else { (top, front) };
    let (reply, words) = status_of(&mut s);
    assert_eq!((words[&first].as_str(), words[&second].as_str()), ("current", "out_of_date"), "{reply}");
    assert_eq!(reply["waiting"], json!(1), "{reply}");
    settled(&mut s, sheet);
    let (doc, projections) = (s.doc.clone(), s.drawing_scenes.projections);
    let (reply, words) = status_of(&mut s);
    assert!(all_are(&words, "current"), "{reply}");
    assert_eq!(reply["waiting"], json!(0), "{reply}");
    assert!(Arc::ptr_eq(&doc, &s.doc) && s.drawing_scenes.projections == projections, "reading draws nothing");

    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(front), "set": {"place": {"at": [160.0, 150.0]}}}));
    assert!(all_are(&status_of(&mut s).1, "current"), "a move marks none");
    run(&mut s, "edit.undo", json!({}));
    assert!(all_are(&status_of(&mut s).1, "current"), "nor its undo");

    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(top), "set": {"tangent_edges": "show"}}));
    let words = status_of(&mut s).1;
    assert_eq!((words[&front].as_str(), words[&top].as_str()), ("current", "out_of_date"));
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(front), "set": {"scale": "1:5"}}));
    assert!(all_are(&status_of(&mut s).1, "out_of_date"), "the view following it too");
    settled(&mut s, sheet);

    let drawing = ids::encode(s.open_drawing.unwrap());
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 40, "corner": [40, 60, 0], "body_name": "Tower", "operation": "new"}));
    run(&mut s, "drawing.open", json!({"drawing": drawing}));
    assert!(all_are(&status_of(&mut s).1, "out_of_date"), "a model edit marks every view");
}

/// A failed or placeholder view always says why, and a view on another sheet is out of date until
/// its own sheet is drawn, with that sheet's id.
#[test]
fn status_says_why_a_view_has_no_lines() {
    let (mut s, sheet, [front, top]) = front_and_top();
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(top), "set": {"visual_style": "shaded_visible"}}));
    settled(&mut s, sheet);
    let (reply, words) = status_of(&mut s);
    assert_eq!((words[&front].as_str(), words[&top].as_str()), ("current", "failed"), "{reply}");
    let failed = reply["views"].as_array().unwrap().iter().find(|v| v["status"] == "failed").unwrap();
    assert!(failed["reason"].as_str().unwrap().contains("exact lines"), "{reply}");
    assert_eq!(failed["sheet"], json!(ids::encode(sheet)), "{reply}");

    run(&mut s, "drawing.sheet.add", json!({"name": "Other"}));
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(top), "set": {"visual_style": null}}));
    run(&mut s, "drawing.view.move_to_sheet", json!({"view": ids::encode(top), "sheet": "Other", "break_alignment": true}));
    settled(&mut s, sheet);
    let (reply, words) = status_of(&mut s);
    assert_eq!((words[&front].as_str(), words[&top].as_str()), ("current", "out_of_date"), "{reply}");
    let other = s.drawing("test").unwrap().sheets.iter().find(|x| x.id != sheet).unwrap().id;
    let moved = reply["views"].as_array().unwrap().iter().find(|v| v["view"] == json!(ids::encode(top))).unwrap();
    assert_eq!(moved["sheet"], json!(ids::encode(other)), "{reply}");

    for level in 0..=MAX_PLACEMENT_DEPTH as u64 + 1 {
        nest(&mut s, level);
    }
    settled(&mut s, sheet);
    let reply = status_of(&mut s).0;
    let held = reply["views"].as_array().unwrap().iter().find(|v| v["view"] == json!(ids::encode(front))).unwrap();
    assert_eq!(held["status"], json!("placeholder"), "{reply}");
    assert!(!held["reason"].as_str().unwrap().is_empty(), "{reply}");
}

/// A view that drew but didn't fit its sheet's scene is failed, as that scene left it, until it is
/// drawn again: a scene older than the last drawing is not trusted.
#[test]
fn status_takes_what_the_last_scene_left_out_until_a_view_draws_again() {
    let (mut s, sheet, [front, top]) = front_and_top();
    settled(&mut s, sheet);
    let fitted = overfill(&mut s, top);
    extract(&mut s, sheet).unwrap();
    let (reply, words) = status_of(&mut s);
    assert_eq!((words[&front].as_str(), words[&top].as_str()), ("current", "failed"), "{reply}");
    let failed = reply["views"].as_array().unwrap().iter().find(|v| v["status"] == "failed").unwrap();
    assert!(failed["reason"].as_str().unwrap().contains("put it on another sheet"), "{reply}");

    // Drawn again since that scene, and it fits: its own drawing says where it stands.
    let cache = &mut s.drawing_scenes;
    cache.serial += 1;
    let entry = cache.views.get_mut(&top).unwrap();
    entry.drawn = fitted;
    entry.serial = cache.serial;
    let (reply, words) = status_of(&mut s);
    assert!(all_are(&words, "current"), "{reply}");
}

/// A scene counts only for its own sheet as that sheet is now: a view moved off the sheet whose
/// scene left it out, and a view left there when another goes, each read their own drawing again.
#[test]
fn status_takes_a_scene_only_for_its_sheet_as_it_is() {
    let failed_on_its_sheet = |s: &mut Session, top: u64| {
        let (reply, words) = status_of(s);
        assert_eq!(words[&top], "failed", "{reply}");
    };
    let (mut s, sheet, [_, top]) = front_and_top();
    settled(&mut s, sheet);
    overfill(&mut s, top);
    extract(&mut s, sheet).unwrap();
    failed_on_its_sheet(&mut s, top);
    run(&mut s, "drawing.sheet.add", json!({"name": "Other"}));
    run(&mut s, "drawing.view.move_to_sheet", json!({"view": ids::encode(top), "sheet": "Other", "break_alignment": true}));
    let (reply, words) = status_of(&mut s);
    assert!(all_are(&words, "current"), "moved, it no longer has the old sheet's advice: {reply}");

    let (mut s, sheet, [front, top]) = front_and_top();
    settled(&mut s, sheet);
    overfill(&mut s, top);
    extract(&mut s, sheet).unwrap();
    failed_on_its_sheet(&mut s, top);
    run(&mut s, "drawing.sheet.add", json!({"name": "Other"}));
    run(&mut s, "drawing.view.move_to_sheet", json!({"view": ids::encode(front), "sheet": "Other", "break_alignment": true}));
    let (reply, words) = status_of(&mut s);
    assert!(all_are(&words, "current"), "its sheet changed since that scene: {reply}");
}

/// Gives `view` a drawing of lines past a paper scene's records on their own (drawn, but no sheet
/// holds them), as if just drawn; returns the drawing it had.
fn overfill(s: &mut Session, view: u64) -> Drawn {
    let cache = &mut s.drawing_scenes;
    cache.serial += 1;
    let serial = cache.serial;
    let entry = cache.views.get_mut(&view).unwrap();
    let path = (0..MAX_RECORDS).map(|i| Seg2::Line { a: Vec2::new(i as f64 * 1e-3, 0.0), b: Vec2::new((i + 1) as f64 * 1e-3, 0.0) }).collect();
    let items = vec![Item::Stroke { path, closed: false, width_mm: 0.5, pattern: None }];
    let group = Group { object: view, kind: ObjectKind::View, items };
    entry.serial = serial;
    std::mem::replace(
        &mut entry.drawn,
        Drawn {
            group: Some(Arc::new(group)),
            status: Status::Drawn,
            stats: None,
            boundary: None,
            frame: None,
            owners: None,
            why: None,
            hatches: Vec::new(),
        },
    )
}

/// Each dimension's word is the one inspect uses: read, then lost when its body goes, with why. Its
/// view not drawn yet, it isn't either: it is out of date. A parameter status takes nothing, and
/// needs a drawing open.
#[test]
fn status_says_where_each_dimension_stands() {
    let (mut s, _, [front, _]) = front_and_top();
    let added = run(&mut s, "drawing.dimension.add", json!({"view": ids::encode(front), "kind": {"length": {"edge": [50, 0, 20]}}}));
    let reply = status_of(&mut s).0;
    assert_eq!(reply["dimensions"], json!([{"dimension": added["record"]["id"], "status": "ok", "out_of_date": true}]), "{reply}");
    let drawing = ids::encode(s.open_drawing.unwrap());
    run(&mut s, "drawing.close", json!({}));
    assert!(s.execute("drawing.status", &json!({})).is_err(), "no drawing open");
    let block = s.doc.features[0].id;
    run(&mut s, "timeline.delete", json!({"feature": block.to_string()}));
    run(&mut s, "drawing.open", json!({"drawing": drawing}));
    let reply = status_of(&mut s).0;
    let lost = &reply["dimensions"][0];
    assert_eq!(lost["status"], json!("lost"), "{reply}");
    assert!(!lost["why"].as_str().unwrap().is_empty(), "{reply}");
    assert!(s.execute("drawing.status", &json!({"sheet": "x"})).is_err());
}

/// The dimensions a `drawing.status` reply says are out of date, by id, in its order.
fn out_of_date_dimensions(reply: &Value) -> Vec<Value> {
    let dimensions = reply["dimensions"].as_array().unwrap().iter();
    dimensions.filter(|d| d["out_of_date"] == json!(true)).map(|d| d["dimension"].clone()).collect()
}

/// A deferred drawing, its front view marked by `mark` (a dimension or a note on it), updated and
/// drawn, then closed while `change` runs on the model and opened again: the session, its sheet and
/// the mark.
fn held_after(mark: fn(&mut Session, u64) -> u64, change: impl FnOnce(&mut Session)) -> (Session, u64, u64) {
    let (mut s, sheet, [front, _]) = front_and_top();
    let id = mark(&mut s, front);
    let drawing = json!({"drawing": ids::encode(s.open_drawing.unwrap())});
    run(&mut s, "drawing.set", json!({"updates": "deferred"}));
    run(&mut s, "drawing.update", json!({}));
    settled(&mut s, sheet);
    run(&mut s, "drawing.close", json!({}));
    change(&mut s);
    run(&mut s, "drawing.open", drawing);
    (s, sheet, id)
}

/// Updated and drawn, a drawing has nothing out of date, and doesn't hold.
fn nothing_waits_once_updated(s: &mut Session, sheet: u64) {
    run(s, "drawing.update", json!({}));
    settled(s, sheet);
    let (reply, words) = status_of(s);
    assert!(all_are(&words, "current") && out_of_date_dimensions(&reply).is_empty() && reply.get("held").is_none(), "{reply}");
}

/// A deferred drawing held by a model change lists, beside its views out of date, their dimension:
/// the sheet shows its last reading too, as `drawing.export` lists it.
#[test]
fn status_lists_a_held_views_dimensions_with_it() {
    let (mut s, sheet, id) = held_after(front_edge, |s| {
        let block = s.doc.features[0].id;
        run(s, "timeline.edit", json!({"feature": block.to_string(), "set": {"length": 120}}));
    });
    let (reply, words) = status_of(&mut s);
    assert!(all_are(&words, "out_of_date"), "{reply}");
    assert_eq!((&reply["held"], &reply["waiting"]), (&json!(true), &json!(2)), "{reply}");
    assert_eq!(out_of_date_dimensions(&reply), [json!(ids::encode(id))], "{reply}");
    assert_eq!(out_of_date_dimensions(&reply), listed(&mut s, sheet)[2..], "as the export lists it, after the views");
    nothing_waits_once_updated(&mut s, sheet);
}

/// A change that shows the same (a sketch: a new world state, no new body) leaves a held drawing's
/// views current, but not their dimension, read from the model before it: listed out of date, with
/// the drawing held though no view waits, as `drawing.export` lists it.
#[test]
fn status_lists_dimensions_out_of_date_whose_views_are_current() {
    let (mut s, sheet, id) = held_after(front_edge, |s| {
        run(s, "sketch.create", json!({"plane": "XY", "name": "Note"}));
        run(s, "sketch.finish", json!({}));
    });
    let (reply, words) = status_of(&mut s);
    assert!(all_are(&words, "current"), "{reply}");
    assert_eq!((&reply["held"], &reply["waiting"]), (&json!(true), &json!(0)), "{reply}");
    assert_eq!(out_of_date_dimensions(&reply), [json!(ids::encode(id))], "{reply}");
    assert_eq!(out_of_date_dimensions(&reply), listed(&mut s, sheet), "as the export lists it");
    nothing_waits_once_updated(&mut s, sheet);
}

/// The same change, with a note on the front view's edge and no dimension: the note was read from the
/// model before it, so the drawing is held though no view waits, as `drawing.export` lists the note;
/// `dimensions` lists none out of date, as it lists dimensions only.
#[test]
fn status_holds_for_a_note_out_of_date_whose_view_is_current() {
    let edge_note = |s: &mut Session, front: u64| view_note(s, front, Some([50.0, 0.0, 0.0]), [60.0, 40.0], json!("Deburr"));
    let (mut s, sheet, note) = held_after(edge_note, |s| {
        run(s, "sketch.create", json!({"plane": "XY", "name": "Note"}));
        run(s, "sketch.finish", json!({}));
    });
    let (reply, words) = status_of(&mut s);
    assert!(all_are(&words, "current") && out_of_date_dimensions(&reply).is_empty(), "{reply}");
    assert_eq!((&reply["held"], &reply["waiting"]), (&json!(true), &json!(0)), "{reply}");
    assert_eq!(listed(&mut s, sheet), [json!(ids::encode(note))], "as the export lists it");
    nothing_waits_once_updated(&mut s, sheet);
}

/// A dimension of the block's top front edge on view `view`, 10 from it; its id.
fn top_edge(s: &mut Session, view: u64) -> u64 {
    let added = run(s, "drawing.dimension.add", json!({"view": ids::encode(view), "kind": {"linear": {"a": [0, 0, 20], "b": [100, 0, 20]}}}));
    ids::decode(added["dimension"].as_str().unwrap()).unwrap()
}

/// `id`'s group in `scene`.
fn group_of(scene: &Scene, id: u64) -> &Group {
    scene.groups.iter().find(|g| g.object == id).unwrap()
}

/// A dimension draws after the views, at paper size and on its view: the top edge of the block at
/// 1:2, centred at (150, 150), has its tips 50 apart, 10 above the edge. `build` and the provider
/// give the same bytes, and its place is listed with the scene's other objects.
#[test]
fn a_dimension_is_drawn_after_the_views_on_the_sheet() {
    let (mut s, sheet, [front, _]) = front_and_top();
    let id = top_edge(&mut s, front);
    let built = built(&s, sheet);
    let group = group_of(&built.scene, id);
    assert_eq!(group.kind, ObjectKind::Dimension);
    assert_eq!(built.scene.groups.last().map(|g| g.object), Some(id));
    let tips: Vec<Vec2> = group
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Fill { region } => Some(region.outer.segs[0].start()),
            _ => None,
        })
        .collect();
    assert_eq!(tips.len(), 2);
    assert!(((tips[0].x - tips[1].x).abs() - 50.0).abs() < 1e-9, "{tips:?}");
    assert!(tips.iter().all(|t| (t.y - 165.0).abs() < 1e-9), "{tips:?}");
    let object = built.objects.iter().find(|o| o.object == id).unwrap();
    assert_eq!(object.kind, ObjectKind::Dimension);
    assert_eq!(
        built.dimensions,
        vec![DimensionState { dimension: id, view: front, status: solidcraft_doc::drawing::dimension::Status::Ok, drawn: true, why: None }]
    );
    let provided = settled(&mut s, sheet);
    assert_eq!(bytes(&provided.scene), bytes(&built.scene));
    assert_eq!(&*provided.objects, built.objects.as_slice());
    assert_eq!(provided.dimensions, built.dimensions);
    assert!(provided.stale_dimensions.is_empty());
}

/// The id under `key` in a command's reply.
fn id_in(reply: Value, key: &str) -> u64 {
    ids::decode(reply[key].as_str().unwrap()).unwrap()
}

/// Every kind of group `drawings` draws, on one sheet, paints as the code put them in by hand before
/// paint bands (#4890): the border, an image, the views by id (shaded, the top view following the
/// front, each picture inside its view's group), the views' dimension and note by view and id, the
/// sheet's own note, then the title block with its logo. `build` and the provider give the same bytes.
#[test]
fn todays_sheets_paint_every_kind_in_the_same_order() {
    let folder = crate::tests::Folder::new("paint-every-kind");
    let (mut s, sheet, [front, top]) = front_and_top();
    let file = folder.path("red-blue.png");
    std::fs::write(&file, crate::drawing_pictures::tests::red_blue()).unwrap();
    let block = serde_json::to_value(solidcraft_geom::title_block::default_block()).unwrap();
    run(&mut s, "drawing.sheet.set", json!({"sheet": ids::encode(sheet), "title_block": block}));
    run(&mut s, "drawing.title_block.logo", json!({"cell": "legal_owner", "file": file}));
    let image = id_in(run(&mut s, "drawing.image.add", json!({"sheet": ids::encode(sheet), "file": file, "rect_mm": [10, 20, 35, 33]})), "image");
    // The sheet's note is added first, so its id doesn't put it last.
    let general =
        id_in(run(&mut s, "drawing.note.add", json!({"anchor": {"sheet": ids::encode(sheet)}, "at": [20, 40], "content": "General"})), "note");
    let dimension = top_edge(&mut s, front);
    let on_front =
        id_in(run(&mut s, "drawing.note.add", json!({"anchor": {"view": ids::encode(front)}, "at": [0, -20], "content": "Front"})), "note");
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(front), "set": {"visual_style": "shaded"}}));

    let mut views = [front, top];
    views.sort_unstable();
    let mut annotations = [(dimension, ObjectKind::Dimension), (on_front, ObjectKind::Note)];
    annotations.sort_by_key(|(id, _)| *id);
    let mut expected: Vec<(u64, ObjectKind)> = border(&s, sheet).iter().map(|g| (g.object, g.kind)).collect();
    expected.push((image, ObjectKind::SheetImage));
    expected.extend(views.map(|view| (view, ObjectKind::View)));
    expected.extend(annotations);
    expected.extend([(general, ObjectKind::Note), (sheet, ObjectKind::TitleBlock)]);
    let built = built(&s, sheet);
    assert_eq!(built.scene.groups.iter().map(|g| (g.object, g.kind)).collect::<Vec<_>>(), expected);
    let pictures = |group: &Group| group.items.iter().filter(|item| matches!(item, Item::Picture(_))).count();
    let block = built.scene.groups.last().unwrap();
    assert!(views.iter().all(|view| pictures(group_of(&built.scene, *view)) > 0) && pictures(block) == 1, "pictures inside their groups");
    assert_eq!(bytes(&settled(&mut s, sheet).scene), bytes(&built.scene));
}

/// Groups paint by their band, not by when they join (#4890): a sheet's note that joins before a
/// view's dimension and note still paints after them, so the scene is `build`'s, byte for byte, and
/// the sheet's objects are listed in the same order, as `hit` reads them.
#[test]
fn a_sheet_note_paints_over_view_annotations_whatever_order_they_join_in() {
    let (mut s, sheet, [front, _]) = front_and_top();
    let dimension = top_edge(&mut s, front);
    let on_front =
        id_in(run(&mut s, "drawing.note.add", json!({"anchor": {"view": ids::encode(front)}, "at": [0, -20], "content": "Front"})), "note");
    let note = id_in(run(&mut s, "drawing.note.add", json!({"anchor": {"sheet": ids::encode(sheet)}, "at": [20, 40], "content": "General"})), "note");
    let d = s.drawing("test").unwrap().clone();
    let (record, on) = (sheet_of(&d, sheet).unwrap(), views_on(&d, sheet).unwrap());
    let shown = crate::drawing_view::shown(&s);
    let lettering = Lettering::of(&s, &d);
    let (text, _) = block_text(record, &lettering, &[], true).unwrap();
    let (notes, _) = sheet_texts(&s, &d, sheet, &lettering, true);
    let mut prepared = PreparedBodies::default();
    let drawn = on.iter().map(|(id, view, centre)| (*id, *centre, draw(&s, &[], &shown, &mut prepared, *id, view)));
    let mut assembly = assemble(record, d.projection, &text, &[], &[], &[], drawn).unwrap();
    let frames: BTreeMap<_, _> = assembly.placed.iter().map(|(view, (frame, _))| (*view, *frame)).collect();
    let line_group = |view: u64| on.iter().find(|(id, ..)| *id == view).map(|(_, v, _)| v.line_group.value);
    let wanted = dimensions_on(&d, on.iter().map(|(id, ..)| *id));
    let dimensions = draw_dimensions(&s, None, &d, &wanted, |view| Some((*frames.get(&view)?, line_group(view)?)), true);
    // The other way round from `build`: the sheet's note joins first.
    assembly.add_sheet_texts(&notes);
    assembly.add_dimensions(dimensions.iter());
    let (scene, _, objects, ..) = assembly.finish().unwrap();
    let mut view_annotations = [dimension, on_front];
    view_annotations.sort_unstable();
    let ours = |object: &u64| [dimension, on_front, note].contains(object);
    let painted: Vec<u64> = scene.groups.iter().map(|g| g.object).filter(ours).collect();
    assert_eq!(painted, [view_annotations[0], view_annotations[1], note]);
    assert_eq!(objects.iter().map(|o| o.object).filter(ours).collect::<Vec<_>>(), painted, "objects in paint order");
    let built = built(&s, sheet);
    assert_eq!((bytes(&scene), &objects), (bytes(&built.scene), &built.objects));
}

/// Moving a view moves its dimensions, the same bytes moved, with no projection and no new read. An
/// edit of one dimension reads that one alone, and projects nothing.
#[test]
fn a_moved_view_moves_its_dimensions_and_an_edit_reads_one() {
    let (mut s, sheet, [front, _]) = front_and_top();
    let id = top_edge(&mut s, front);
    let other = run(&mut s, "drawing.dimension.add", json!({"view": ids::encode(front), "kind": {"linear": {"a": [0, 0, 0], "b": [0, 0, 20]}}}));
    let other = ids::decode(other["dimension"].as_str().unwrap()).unwrap();
    let before = settled(&mut s, sheet);
    let (projections, serial) = (s.drawing_scenes.projections, s.drawing_scenes.dimensions[&other].serial);

    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(front), "set": {"place": {"at": [170.0, 130.0]}}}));
    let after = settled(&mut s, sheet);
    assert_eq!(s.drawing_scenes.projections, projections, "a move projects nothing");
    assert_eq!(s.drawing_scenes.dimensions[&other].serial, serial, "and reads nothing");
    assert_eq!(bytes_of(group_of(&after.scene, id)), bytes_of(&moved(group_of(&before.scene, id), Vec2::new(20.0, -20.0))));

    run(&mut s, "drawing.dimension.edit", json!({"dimension": ids::encode(id), "set": {"offset_mm": 20.0}}));
    let edited = settled(&mut s, sheet);
    assert_eq!(s.drawing_scenes.projections, projections, "an edit projects nothing");
    assert_eq!(s.drawing_scenes.dimensions[&other].serial, serial, "the other dimension isn't read");
    assert_ne!(bytes_of(group_of(&edited.scene, id)), bytes_of(group_of(&after.scene, id)));
    assert_eq!(bytes(&edited.scene), bytes(&built(&s, sheet).scene));
}

fn bytes_of(group: &Group) -> String {
    serde_json::to_string(group).unwrap()
}

/// The block with a hole 12 across through its front, and a dimension of the hole on the front view:
/// the session, the sheet, the view, the dimension and the drawing's id.
fn holed() -> (Session, u64, u64, u64, String) {
    let (mut s, sheet, [front, _]) = front_and_top();
    let drawing = ids::encode(s.open_drawing.unwrap());
    run(&mut s, "drawing.close", json!({}));
    let cut = json!({"radius": 6, "height": 52, "base": [50, -1, 10], "axis": "Y", "operation": "cut", "targets": ["Block"], "name": "Hole"});
    run(&mut s, "solid.cylinder", cut);
    run(&mut s, "drawing.open", json!({"drawing": drawing}));
    let added = run(&mut s, "drawing.dimension.add", json!({"view": ids::encode(front), "kind": {"radial": {"edge": [56, 0, 10]}}}));
    (s, sheet, front, ids::decode(added["dimension"].as_str().unwrap()).unwrap(), drawing)
}

/// The hole deleted, with the drawing closed as a model edit has it.
fn delete_hole(s: &mut Session, drawing: &str) {
    run(s, "drawing.close", json!({}));
    run(s, "timeline.delete", json!({"feature": "Hole"}));
    run(s, "drawing.open", json!({"drawing": drawing}));
}

/// A dimension whose view isn't drawn isn't drawn and says so. A lost one has no lines, is listed,
/// and leaves the sheet drawn; undo finds it again, the same bytes. A model edit that moves what it
/// measures moves its lines.
#[test]
fn a_lost_dimension_leaves_the_sheet_drawn_and_is_listed() {
    let (mut s, sheet, [front, top]) = front_and_top();
    let on_top = top_edge(&mut s, top);
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(top), "set": {"visual_style": "shaded_visible"}}));
    let view_failed = settled(&mut s, sheet);
    let held = view_failed.dimensions.iter().find(|d| d.dimension == on_top).unwrap();
    assert!(!held.drawn && held.why.as_deref() == Some("its view isn't drawn"), "{held:?}");
    assert!(view_failed.scene.groups.iter().all(|g| g.object != on_top));
    assert_eq!(bytes(&view_failed.scene), bytes(&built(&s, sheet).scene));

    // Taller: the view is drawn again first, then its dimension, at the new place.
    let id = top_edge(&mut s, front);
    let before = settled(&mut s, sheet);
    let drawing = ids::encode(s.open_drawing.unwrap());
    run(&mut s, "drawing.close", json!({}));
    let block = s.doc.features[0].id;
    run(&mut s, "timeline.edit", json!({"feature": block.to_string(), "set": {"height": 40}}));
    run(&mut s, "drawing.open", json!({"drawing": drawing}));
    let view = extract(&mut s, sheet).unwrap();
    assert!(view.stale.contains(&front) && view.stale_dimensions.is_empty(), "the views first");
    assert_ne!(bytes_of(group_of(&settled(&mut s, sheet).scene, id)), bytes_of(group_of(&before.scene, id)));

    let (mut s, sheet, front, id, drawing) = holed();
    let before = settled(&mut s, sheet);
    delete_hole(&mut s, &drawing);
    let lost = settled(&mut s, sheet);
    let state = lost.dimensions.iter().find(|d| d.dimension == id).unwrap();
    assert!(!state.drawn && matches!(state.status, solidcraft_doc::drawing::dimension::Status::Lost(..)), "{state:?}");
    assert!(lost.scene.groups.iter().all(|g| g.object != id));
    assert!(lost.scene.groups.iter().any(|g| g.object == front), "the sheet is still drawn");
    assert_eq!(bytes(&lost.scene), bytes(&built(&s, sheet).scene));

    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "drawing.open", json!({"drawing": drawing}));
    let again = settled(&mut s, sheet);
    assert_eq!(bytes_of(group_of(&again.scene, id)), bytes_of(group_of(&before.scene, id)), "found again, drawn the same");
}

/// A dimension past a paper scene's records is left out and says so, and the rest of the sheet is
/// still drawn. So is one whose lines no paper scene holds.
#[test]
fn a_dimension_past_the_scenes_limits_is_left_out_and_says_why() {
    let (s, sheet, [front, _]) = front_and_top();
    let d = s.drawing("test").unwrap().clone();
    let record = sheet_of(&d, sheet).unwrap();
    let group = |items| Group { object: front, kind: ObjectKind::View, items };
    let view = Drawn {
        hatches: Vec::new(),
        group: Some(Arc::new(group(Vec::new()))),
        status: Status::Drawn,
        stats: None,
        boundary: None,
        frame: Some(ViewFrame::new(solidcore_record::Plane::XY, "1:1".parse().unwrap(), Vec2::ZERO, Vec2::ZERO).unwrap()),
        owners: None,
        why: None,
    };
    let mut assembly =
        assemble(record, Projection::Third, &labels(record), &[], &[], &[], std::iter::once((front, Vec2::new(100.0, 100.0), view))).unwrap();
    let stroke = |to: Vec2| Item::Stroke { path: vec![Seg2::Line { a: Vec2::ZERO, b: to }], closed: false, width_mm: 0.25, pattern: None };
    let dimension = |id, items| DrawnDimension {
        dimension: id,
        view: front,
        kind: ObjectKind::Dimension,
        row: None,
        status: solidcraft_doc::drawing::dimension::Status::Ok,
        group: Some(Arc::new(Group { object: id, kind: ObjectKind::Dimension, items })),
        fonts: Vec::new(),
        why: None,
        pending: false,
    };
    let too_many = dimension(1, vec![stroke(Vec2::X); MAX_RECORDS + 1]);
    let off_paper = dimension(2, vec![stroke(Vec2::new(1e9, 0.0))]);
    let fits = dimension(3, vec![stroke(Vec2::X)]);
    assembly.add_dimensions([&too_many, &off_paper, &fits].into_iter());
    let (scene, _, objects, states, _, _) = assembly.finish().unwrap();
    assert!(!states[0].drawn && states[0].why.as_deref().is_some_and(|w| w.contains("records")), "{:?}", states[0]);
    assert!(!states[1].drawn && states[1].why.is_some(), "{:?}", states[1]);
    assert!(states[2].drawn && states[2].why.is_none(), "{:?}", states[2]);
    assert!(scene.groups.iter().any(|g| g.object == 3) && !scene.groups.iter().any(|g| g.object == 1 || g.object == 2));
    assert!(objects.iter().any(|o| o.object == 3 && o.kind == ObjectKind::Dimension));
}

/// Two dimensions' text shares one font table holding the glyphs of both, a dimension left out adds
/// none of its own, and a font's records count: the group that would pass the limit only with its
/// font is left out. The scene takes what joined.
#[test]
fn dimensions_share_one_font_table_and_one_left_out_adds_nothing() {
    let (s, sheet, [front, _]) = front_and_top();
    let d = s.drawing("test").unwrap().clone();
    let record = sheet_of(&d, sheet).unwrap();
    let frame = ViewFrame::new(solidcore_record::Plane::XY, "1:1".parse().unwrap(), Vec2::ZERO, Vec2::ZERO).unwrap();
    let view = Drawn {
        hatches: Vec::new(),
        group: Some(Arc::new(Group { object: front, kind: ObjectKind::View, items: Vec::new() })),
        status: Status::Drawn,
        stats: None,
        boundary: None,
        frame: Some(frame),
        owners: None,
        why: None,
    };
    let text = |id, text: &str, more: usize| {
        let style = solidcraft_sketch::TextStyle::default();
        let (runs, fonts) = solidcraft_sketch::paper_text(text, None, "", Vec2::ZERO, 3.5, 0.0, &style).unwrap().unwrap();
        let line = Item::Stroke { path: vec![Seg2::Line { a: Vec2::ZERO, b: Vec2::X }], closed: false, width_mm: 0.25, pattern: None };
        let items = runs.into_iter().map(Item::Text).chain(std::iter::repeat_n(line, more)).collect();
        DrawnDimension {
            dimension: id,
            view: front,
            kind: ObjectKind::Dimension,
            row: None,
            status: solidcraft_doc::drawing::dimension::Status::Ok,
            group: Some(Arc::new(Group { object: id, kind: ObjectKind::Dimension, items })),
            fonts,
            why: None,
            pending: false,
        }
    };
    let glyphs = |one: &DrawnDimension| one.fonts[0].glyphs.len();
    let (a, b, too_big) = (text(1, "11", 0), text(2, "22", 0), text(3, "77", MAX_RECORDS));
    let shared = glyphs(&a) + glyphs(&b);
    let mut assembly =
        assemble(record, Projection::Third, &labels(record), &[], &[], &[], std::iter::once((front, Vec2::new(100.0, 100.0), view.clone()))).unwrap();
    assembly.add_dimensions([&a, &too_big, &b].into_iter());
    let (scene, _, objects, states, _, _) = assembly.finish().unwrap();
    assert!(states[0].drawn && !states[1].drawn && states[2].drawn, "{states:?}");
    assert_eq!(scene.fonts.len(), 1);
    assert_eq!(scene.fonts[0].glyphs.len(), shared, "its 7 stays out");
    // The text moved with its group, and its box is the box round its glyphs.
    let bounds = objects.iter().find(|o| o.object == 1).unwrap().bounds_mm;
    assert!(bounds.min.x >= 100.0 && bounds.min.y >= 100.0 && bounds.max.y > 102.0 && bounds.max.y < 104.0, "{bounds:?}");

    // Past the limit only with its font's records: its group alone would fit. Without them, it joins.
    let mut assembly =
        assemble(record, Projection::Third, &labels(record), &[], &[], &[], std::iter::once((front, Vec2::new(100.0, 100.0), view))).unwrap();
    let room = MAX_RECORDS - assembly.records;
    let records = |one: &DrawnDimension| one.group.as_ref().unwrap().records();
    let (bare, per_line) = (text(4, "8", 0), records(&text(4, "8", 1)) - records(&text(4, "8", 0)));
    let fonts = assembly.scene.font_records_beyond(&bare.fonts);
    let over = text(4, "8", (room - records(&bare)) / per_line);
    let fits = text(5, "8", (room - records(&bare) - fonts) / per_line);
    assert!(records(&over) <= room);
    assembly.add_dimensions([&over, &fits].into_iter());
    let (_, _, _, states, _, _) = assembly.finish().unwrap();
    assert!(!states[0].drawn && states[0].why.as_deref().is_some_and(|why| why.contains("records")), "{:?}", states[0]);
    assert!(states[1].drawn, "{:?}", states[1]);
}

/// A dimension's text is in the scene's reply as its schema says, with the font it names and never
/// the font's bytes; `build` and the provider give the same bytes, and an export writes it.
#[test]
fn a_dimensions_text_is_in_the_reply_and_the_export() {
    let (mut s, sheet, [front, _]) = front_and_top();
    let id = top_edge(&mut s, front);
    let reply = run(&mut s, "drawing.scene", json!({"sheet": ids::encode(sheet)}));
    let schema = output_schema_for::<SheetSceneResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &reply), "{reply}");
    let groups = reply["scene"]["groups"].as_array().unwrap();
    let group = groups.iter().find(|g| g["object"] == json!(id)).unwrap();
    let text = group["items"].as_array().unwrap().iter().find(|item| item["kind"] == json!("text")).unwrap();
    assert_eq!(text["text"], json!("100.00"), "{text}");
    assert_eq!(reply["scene"]["fonts"][0]["id"], text["font"]);
    assert!(reply["scene"]["fonts"][0].get("bytes").is_none());
    assert_eq!(bytes(&settled(&mut s, sheet).scene), bytes(&built(&s, sheet).scene));
    let exported = run(&mut s, "drawing.export", json!({"sheet": ids::encode(sheet)}));
    assert_eq!(exported["dimensions"][0]["drawn"], json!(true), "{exported}");
}

/// A dimension's text is set in its drawing's language and font: changing either reads it again,
/// as changing the units does.
#[test]
fn a_new_language_or_font_reads_the_dimensions_again() {
    let (mut s, sheet, [front, _]) = front_and_top();
    let id = top_edge(&mut s, front);
    settled(&mut s, sheet);
    assert!(extract(&mut s, sheet).unwrap().stale_dimensions.is_empty());
    for change in [json!({"language": "de"}), json!({"font": solidcraft_sketch::DEFAULT_FONT_FAMILY})] {
        run(&mut s, "drawing.set", change.clone());
        assert_eq!(extract(&mut s, sheet).unwrap().stale_dimensions, vec![id], "{change}");
        let again = settled(&mut s, sheet);
        assert!(again.stale_dimensions.is_empty() && again.dimensions.iter().any(|d| d.dimension == id && d.drawn), "{change}");
    }
}

/// The scene's reply lists its dimensions in its schema, with a dimension's group in the scene, and
/// an export writes the sheet with a lost dimension and reports it.
#[test]
fn replies_list_the_dimensions_and_an_export_reports_a_lost_one() {
    let (mut s, sheet, [front, _]) = front_and_top();
    let id = top_edge(&mut s, front);
    let reply = run(&mut s, "drawing.scene", json!({"sheet": ids::encode(sheet)}));
    let schema = output_schema_for::<SheetSceneResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &reply), "{reply}");
    assert_eq!(reply["dimensions"], json!([{"dimension": ids::encode(id), "view": ids::encode(front), "status": "ok", "drawn": true}]));
    let groups = reply["scene"]["groups"].as_array().unwrap();
    assert_eq!(groups.last().unwrap()["kind"], json!("dimension"), "{reply}");
    let exported = run(&mut s, "drawing.export", json!({"sheet": ids::encode(sheet)}));
    assert_eq!(exported["dimensions"][0]["drawn"], json!(true), "{exported}");

    let (mut s, sheet, _, _, drawing) = holed();
    delete_hole(&mut s, &drawing);
    let exported = run(&mut s, "drawing.export", json!({"sheet": ids::encode(sheet)}));
    assert_eq!((&exported["dimensions"][0]["drawn"], &exported["dimensions"][0]["status"]), (&json!(false), &json!("lost")), "{exported}");
    assert!(exported["svg"].as_str().is_some_and(|svg| svg.contains("<svg")), "still written");
}

/// A saved view draws what it shows: with the second copy hidden, one cube's outline, 10 mm across
/// and 40 mm of line, where both copies are 40 mm across. The provider sees the change as it sees
/// any other setting's, and ends at the built scene; a view drawing nothing is left out and says why.
#[test]
fn a_view_draws_what_it_shows() {
    let (mut s, [first, second]) = two_cubes();
    run(&mut s, "drawing.create", json!({}));
    let sheet = s.drawing("test").unwrap().sheets[0].id;
    let made = run(&mut s, "drawing.view.base", json!({"sheet": ids::encode(sheet), "scale": "1:1", "position_mm": [150, 150]}));
    let view = ids::decode(made["view"].as_str().unwrap()).unwrap();
    let group = |scene: &Scene| scene.groups.iter().find(|g| g.object == view && g.kind == ObjectKind::View).cloned();
    let across = |scene: &Scene| {
        let (lo, hi, length) = reach(&group(scene).unwrap());
        (hi.x - lo.x, length)
    };

    let both = settled(&mut s, sheet);
    assert!((across(&both.scene).0 - 40.0).abs() < 1e-6, "{:?}", across(&both.scene));
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(view), "set": {"hide": [[second]]}}));
    let waiting = extract(&mut s, sheet).unwrap();
    assert_eq!((waiting.stale.as_slice(), bytes(&waiting.scene)), ([view].as_slice(), bytes(&both.scene)), "the last lines show until it is drawn");

    let one = settled(&mut s, sheet);
    let (wide, length) = across(&one.scene);
    assert!((wide - 10.0).abs() < 1e-6 && (length - 40.0).abs() < 1e-6, "{wide} {length}");
    assert_eq!(bytes(&one.scene), bytes(&built(&s, sheet).scene));
    let left = group(&one.scene).unwrap();
    let (lo, hi, _) = reach(&left);
    assert!((lo.x - 145.0).abs() < 1e-6 && (hi.x - 155.0).abs() < 1e-6, "its middle on its centre: {lo:?} {hi:?}");

    // The record allows a choice no occurrence is left in; a deleted one comes to the same.
    let mut d = (**s.drawing("test").unwrap()).clone();
    let at = d.views.iter().position(|v| v.id == view).unwrap();
    Arc::make_mut(&mut d.views[at]).settings.occurrences = Some(vec![vec![first]].into());
    Arc::make_mut(&mut d.views[at]).settings.hide = Some(vec![vec![first]].into());
    s.doc_mut().put_drawing(Arc::new(d));
    let scene = built(&s, sheet);
    assert_eq!(scene.views[0].status, Status::Placeholder(Unmeasured::NothingChosen));
    assert!(group(&scene.scene).is_none(), "left out, the sheet still draws");
    assert!(Unmeasured::NothingChosen.to_string().contains("may have been deleted"));
}

/// An object of a scene with `[min, max]` as its box: a view's boundary, or another object's bounds.
fn object_at(object: u64, kind: ObjectKind, min: [f64; 2], max: [f64; 2]) -> SheetObject {
    let b = Aabb2 { min: Vec2::from(min), max: Vec2::from(max) };
    let view = kind == ObjectKind::View;
    SheetObject { object, kind, bounds_mm: if view { Aabb2::EMPTY } else { b }, boundary_mm: view.then_some(b) }
}

/// The smallest box holding the point wins, then the topmost: nested, overlapping and equal boxes,
/// exactly on an edge, within a band and just past it. The sheet's border is never hit, and a point or
/// band that isn't a number hits nothing.
#[test]
fn a_point_hits_the_smallest_box_then_the_topmost() {
    let big = object_at(1, ObjectKind::View, [0.0, 0.0], [100.0, 100.0]);
    let small = object_at(2, ObjectKind::View, [40.0, 40.0], [60.0, 60.0]);
    let dimension = object_at(3, ObjectKind::Dimension, [45.0, 45.0], [55.0, 50.0]);
    let border = object_at(9, ObjectKind::DrawingBorder, [-10.0, -10.0], [500.0, 500.0]);
    let at = |x: f64, y: f64| Vec2::new(x, y);
    let id = |objects: &[SheetObject], p: Vec2, band: f64| hit(objects, p, band).map(|h| h.object);

    let nested = [border, big, small, dimension];
    assert_eq!(id(&nested, at(50.0, 47.0), 0.0), Some(3), "the dimension on the small view");
    assert_eq!(id(&nested, at(50.0, 55.0), 0.0), Some(2));
    assert_eq!(id(&nested, at(10.0, 10.0), 0.0), Some(1));
    assert_eq!(id(&nested, at(200.0, 200.0), 0.0), None, "the border is no object to pick");
    assert_eq!(hit(&nested, at(50.0, 47.0), 0.0).map(|h| h.kind), Some(ObjectKind::Dimension));
    // Whichever order they paint in, the smaller wins.
    assert_eq!(id(&[small, big], at(50.0, 50.0), 0.0), Some(2));
    assert_eq!(id(&[big, small], at(50.0, 50.0), 0.0), Some(2));
    // Two the same size: the one painted last.
    let twin = object_at(4, ObjectKind::View, [40.0, 40.0], [60.0, 60.0]);
    assert_eq!(id(&[small, twin], at(50.0, 50.0), 0.0), Some(4));
    assert_eq!(id(&[twin, small], at(50.0, 50.0), 0.0), Some(2));
    // A box only the band reaches still counts, and the smaller of the two wins.
    assert_eq!(id(&[big, small], at(39.0, 50.0), 2.0), Some(2));

    let on = |p: Vec2, band: f64| hit(&[small], p, band).map(|h| h.on_edge);
    assert_eq!(on(at(40.0, 50.0), 0.0), Some(true), "exactly on an edge, with no band");
    assert_eq!(on(at(60.0, 60.0), 0.0), Some(true), "a corner");
    assert_eq!(on(at(50.0, 50.0), 0.0), Some(false));
    assert_eq!(on(at(39.0, 50.0), 2.0), Some(true), "outside, within the band");
    assert_eq!(on(at(50.0, 62.0), 2.0), Some(true), "outside on the far side, at the band's end");
    assert_eq!(on(at(37.0, 50.0), 2.0), None);
    assert_eq!(on(at(42.0, 50.0), 2.0), Some(true), "inside, within the band");
    assert_eq!(on(at(43.0, 50.0), 2.0), Some(false));

    for (p, band) in [
        (at(f64::NAN, 50.0), 0.0),
        (at(50.0, f64::INFINITY), 0.0),
        (at(50.0, 50.0), f64::NAN),
        (at(50.0, 50.0), f64::INFINITY),
        (at(50.0, 50.0), -1.0),
    ] {
        assert_eq!(on(p, band), None, "{p:?} {band}");
    }
    assert_eq!(hit(&[], at(0.0, 0.0), 0.0), None);
}

/// The command answers from the sheet as drawn, evaluating nothing: a view, a dimension on it, and
/// nothing. A view that can't be drawn is still there to pick, and a band below 0, a band that isn't a
/// number and an unknown sheet are refused.
#[test]
fn hit_says_what_is_under_a_point_of_a_sheet() {
    let (mut s, sheet, [front, top]) = front_and_top();
    let dimension = top_edge(&mut s, front);
    let sheet_id = ids::encode(sheet);
    let evaluations = s.evaluations;
    let ask =
        |s: &mut Session, point: [f64; 2], band: f64| run(s, "drawing.scene.hit", json!({"sheet": sheet_id, "point_mm": point, "band_mm": band}));
    let schema = output_schema_for::<crate::schema::drawing_scene::SheetHitResult>();

    let on_view = ask(&mut s, [150.0, 150.0], 0.0);
    assert!(conforms(schema.as_value(), schema.as_value(), &on_view), "{on_view}");
    assert_eq!(on_view["hit"], json!({"object": ids::encode(front), "kind": "view", "on_edge": false}));
    let on_dimension = ask(&mut s, [150.0, 165.0], 0.0);
    assert_eq!((&on_dimension["hit"]["object"], &on_dimension["hit"]["kind"]), (&json!(ids::encode(dimension)), &json!("dimension")));
    assert_eq!(ask(&mut s, [124.5, 150.0], 1.0)["hit"]["on_edge"], json!(true), "just outside the front view's left side, in the band");
    assert_eq!(ask(&mut s, [124.5, 150.0], 0.0)["hit"], Value::Null);
    let nothing = ask(&mut s, [10.0, 10.0], 0.0);
    assert!(conforms(schema.as_value(), schema.as_value(), &nothing), "{nothing}");
    assert_eq!(nothing["hit"], Value::Null);
    assert_eq!(s.evaluations, evaluations);

    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(top), "set": {"visual_style": "shaded_visible"}}));
    assert_eq!(ask(&mut s, [150.0, 220.0], 0.0)["hit"]["object"], json!(ids::encode(top)), "a view that can't be drawn");
    let default_band = run(&mut s, "drawing.scene.hit", json!({"sheet": sheet_id, "point_mm": [150.0, 150.0]}));
    assert_eq!(default_band["hit"]["object"], json!(ids::encode(front)));
    for band in [json!(-1.0), Value::Null, json!("2")] {
        let p = json!({"sheet": sheet_id, "point_mm": [150.0, 150.0], "band_mm": band});
        assert!(s.execute("drawing.scene.hit", &p).is_err(), "{band}");
    }
    assert!(s.execute("drawing.scene.hit", &json!({"sheet": "no such", "point_mm": [0.0, 0.0]})).is_err());
    assert!(s.execute("drawing.scene.hit", &json!({"sheet": sheet_id, "point_mm": [0.0]})).is_err());
}

/// A model change with the drawing open marks the view, and its old lines keep their old dimensions
/// until the one step that swaps both: at no step do new dimensions sit on old lines or old ones on
/// new. The dimension's points land on the view's new lines, and a dimension's own edit projects
/// nothing.
#[test]
fn a_views_dimensions_swap_with_its_lines() {
    let (mut s, sheet, [front, _]) = front_and_top();
    let id = top_edge(&mut s, front);
    let drawing = ids::encode(s.open_drawing.unwrap());
    let checkpoint = run(&mut s, "edit.checkpoint", json!({}))["checkpoint"].clone();
    run(&mut s, "drawing.close", json!({}));
    let block = s.doc.features[0].id;
    run(&mut s, "timeline.edit", json!({"feature": block.to_string(), "set": {"height": 40}}));
    run(&mut s, "drawing.open", json!({"drawing": drawing}));
    let old = settled(&mut s, sheet);
    // Back to 20 tall, with the drawing open.
    run(&mut s, "edit.restore_checkpoint", json!({"checkpoint": checkpoint}));

    let waiting = extract(&mut s, sheet).unwrap();
    assert_eq!(bytes_of(group_of(&waiting.scene, id)), bytes_of(group_of(&old.scene, id)), "the old lines keep the old dimension");
    assert_eq!(bytes_of(group_of(&waiting.scene, front)), bytes_of(group_of(&old.scene, front)));

    // A step at a time, until the front view is drawn: until then its dimension stays on its old lines.
    let one = loop {
        step(&mut s);
        let mid = extract(&mut s, sheet).unwrap();
        if !mid.stale.contains(&front) {
            break mid;
        }
        assert_eq!(bytes_of(group_of(&mid.scene, id)), bytes_of(group_of(&old.scene, id)), "the old lines keep the old dimension");
    };
    let new = built(&s, sheet);
    assert_ne!(bytes_of(group_of(&old.scene, front)), bytes_of(group_of(&new.scene, front)), "the model changed the view");
    assert_eq!(bytes_of(group_of(&one.scene, front)), bytes_of(group_of(&new.scene, front)));
    assert_eq!(bytes_of(group_of(&one.scene, id)), bytes_of(group_of(&new.scene, id)), "and its dimension in the same swap");
    assert!(one.stale_dimensions.is_empty(), "{:?}", one.stale_dimensions);

    // The block's top edge at 1:2, as a_dimension_is_drawn_after_the_views_on_the_sheet has it: 10 above it.
    let tips: Vec<f64> = group_of(&one.scene, id)
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Fill { region } => Some(region.outer.segs[0].start().y),
            _ => None,
        })
        .collect();
    assert!(tips.iter().all(|y| (y - 165.0).abs() < 1e-9), "{tips:?}");

    let end = settled(&mut s, sheet);
    let projections = s.drawing_scenes.projections;
    run(&mut s, "drawing.dimension.edit", json!({"dimension": ids::encode(id), "set": {"offset_mm": 20.0}}));
    let edited = settled(&mut s, sheet);
    assert_eq!(s.drawing_scenes.projections, projections);
    assert_eq!(bytes_of(group_of(&edited.scene, front)), bytes_of(group_of(&end.scene, front)));
    assert_ne!(bytes_of(group_of(&edited.scene, id)), bytes_of(group_of(&end.scene, id)));
}

/// The nearest stroke to a point: on a line, on a whole circle from outside and inside, none past
/// the band or a line's end, the one painted last when two are as near, and a picture passed over.
#[test]
fn a_point_finds_the_nearest_stroke_within_the_band() {
    let stroke = |seg: Seg2| Item::Stroke { path: vec![seg], closed: false, width_mm: 0.5, pattern: None };
    let circle = Seg2::Arc { center: Vec2::new(0.0, 0.0), radius: 5.0, start: 0.0, sweep: std::f64::consts::TAU };
    let line = Seg2::Line { a: Vec2::new(10.0, 0.0), b: Vec2::new(10.0, 10.0) };
    let picture = Item::Picture(Picture { source: 0, rect: Aabb2 { min: Vec2::new(-20.0, -20.0), max: Vec2::new(20.0, 20.0) }, opacity: 1.0 });
    let items = [picture, stroke(circle), stroke(line), stroke(line)];
    let near = |x: f64, y: f64, band: f64| nearest_stroke(&items, Vec2::new(x, y), band);
    let close = |got: Option<(usize, f64)>, at: usize, distance: f64| got.is_some_and(|(a, d)| a == at && (d - distance).abs() < 1e-6);
    assert!(close(near(5.25, 0.0, 0.5), 1, 0.25), "outside the circle");
    assert!(close(near(0.0, 4.5, 0.5), 1, 0.5), "inside it, at the band");
    assert_eq!(near(0.0, 0.0, 0.5), None, "its centre is 5 mm from it, and the picture is no line");
    assert!(close(near(9.5, 5.0, 1.0), 3, 0.5), "the second of two as near");
    assert_eq!(near(10.0, 10.75, 0.5), None, "past a line's end");
    assert!(close(near(10.0, 10.0, 0.0), 3, 0.0), "on it, with no band");
    assert_eq!(near(f64::NAN, 0.0, 1.0), None);
}

fn line_under(s: &mut Session, sheet: u64, point: [f64; 2], band: f64) -> Value {
    let reply = run(s, "drawing.scene.line", json!({"sheet": ids::encode(sheet), "point_mm": point, "band_mm": band}));
    let schema = output_schema_for::<crate::schema::drawing_scene::SheetLineResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &reply), "{reply}");
    reply["line"].clone()
}

/// The front view at 1:2 fills 125..175 x 145..155 round (150, 150): a press near its top edge names
/// the edge, 0.2 mm away; one in its middle is 5 mm from any line; a moved view takes its lines along
/// and draws nothing; a view that can't be drawn has no lines. Nothing is evaluated, or changed.
#[test]
fn a_press_names_the_line_under_it() {
    let (mut s, sheet, [front, top]) = front_and_top();
    let evaluations = s.evaluations;
    let doc = s.doc.clone();
    let line = line_under(&mut s, sheet, [150.0, 155.2], 0.5);
    assert_eq!(
        (&line["view"], &line["body"], &line["occurrence"], &line["reference"]),
        (&json!(ids::encode(front)), &json!("Block"), &json!([]), &json!(false)),
        "{line}"
    );
    assert_eq!(line["source"]["kind"], "edge", "{line}");
    assert!((line["distance_mm"].as_f64().unwrap() - 0.2).abs() < 1e-6, "{line}");
    assert_eq!(line_under(&mut s, sheet, [150.0, 150.0], 0.5), Value::Null);
    let wide = line_under(&mut s, sheet, [150.0, 150.0], 6.0);
    assert!((wide["distance_mm"].as_f64().unwrap() - 5.0).abs() < 1e-6, "{wide}");
    assert_eq!(line_under(&mut s, sheet, [10.0, 10.0], 1.0), Value::Null);
    let default_band = run(&mut s, "drawing.scene.line", json!({"sheet": ids::encode(sheet), "point_mm": [150.0, 155.9]}));
    assert_eq!(default_band["line"]["view"], json!(ids::encode(front)), "1 mm by default: {default_band}");
    assert_eq!(s.evaluations, evaluations);
    assert!(Arc::ptr_eq(&doc, &s.doc));

    let projections = s.drawing_scenes.projections;
    line_under(&mut s, sheet, [150.0, 155.2], 0.5);
    assert_eq!(s.drawing_scenes.projections, projections, "a press draws nothing");
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(front), "set": {"place": {"at": [160.0, 150.0]}}}));
    assert_eq!(line_under(&mut s, sheet, [160.0, 155.2], 0.5)["view"], json!(ids::encode(front)));
    assert_eq!(s.drawing_scenes.projections, projections, "nor does a move");

    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(top), "set": {"visual_style": "shaded_visible"}}));
    assert_eq!(line_under(&mut s, sheet, [160.0, 220.0], 40.0), Value::Null);
    for band in [json!(-1.0), json!("wide"), Value::Null] {
        assert!(s.execute("drawing.scene.line", &json!({"sheet": ids::encode(sheet), "point_mm": [0.0, 0.0], "band_mm": band})).is_err(), "{band}");
    }
    assert!(s.execute("drawing.scene.line", &json!({"sheet": "no such", "point_mm": [0.0, 0.0]})).is_err());
}

/// A press names the part a line is of, and says when that part is reference: part `over`'s top edge
/// lies at z 20 and x 10..30, drawn with the percent taking it in.
#[test]
fn a_press_names_the_part_and_its_role() {
    let (design, over) = under_and_over();
    let (mut s, sheet, [front, _]) = front_and_top_of(design);
    // The two parts span x 0..30 and z 0..20, round (150, 150) at 1:2: the edge at y 155.
    let line = line_under(&mut s, sheet, [152.0, 155.1], 0.5);
    assert_eq!((&line["occurrence"], &line["reference"], &line["body"]), (&json!([over]), &json!(false), &json!("BodyOver")), "{line}");
    let drawing = json!({"drawing": ids::encode(s.drawing("test").unwrap().id)});
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "occurrence.bom_type", json!({"occurrence": over, "bom_type": "reference"}));
    run(&mut s, "drawing.open", drawing);
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(front), "set": {"reference_shown_percent": 100}}));
    // The extents are now the part under it, x 0..20 and z 0..10, so z 20 is at y 157.5.
    let line = line_under(&mut s, sheet, [155.0, 157.6], 0.5);
    assert_eq!((&line["occurrence"], &line["reference"]), (&json!([over]), &json!(true)), "{line}");
}

/// A press names what drew a line, on each placement of a part placed twice: a round bar of radius
/// 10 and height 20 along Z, the second 40 along X. Its outline is the bar's silhouette, by the face
/// it lies on, and its top is an edge, each through the occurrence pressed. At 1:2 round (150, 150)
/// the two span x -10..50 and z 0..20: the far sides at x 135 and 165, the tops at y 155.
#[test]
fn a_press_names_what_drew_the_line_on_each_placement() {
    let mut design = Session::default();
    let first = run(&mut design, "component.create", json!({"name": "Bar"}))["occurrence"].as_u64().unwrap();
    run(&mut design, "solid.cylinder", json!({"radius": 10, "height": 20, "base": [0, 0, 0], "axis": "Z", "body_name": "Round", "operation": "new"}));
    run(&mut design, "component.activate", json!({"component": 0}));
    let second = run(&mut design, "occurrence.copy", json!({"component": "Bar", "translate": [40, 0, 0]}))["occurrence"].as_u64().unwrap();
    let (mut s, sheet, _) = front_and_top_of(design);
    let side = |s: &Session, x: f64| {
        let state = s.model.state();
        let body = state.body("Round").unwrap();
        solidcraft_doc::appearance::face_index_at(body, solidcore_record::Vec3::new(x, 0.0, 10.0)).unwrap()
    };
    let (left, right) = (side(&s, -10.0), side(&s, 10.0));
    for (x, occurrence, face) in [(135.1, first, left), (164.9, second, right)] {
        let line = line_under(&mut s, sheet, [x, 150.0], 0.5);
        assert_eq!((&line["body"], &line["occurrence"]), (&json!("Round"), &json!([occurrence])), "{line}");
        assert_eq!((&line["source"]["kind"], &line["source"]["face"]), (&json!("silhouette"), &json!(face)), "{line}");
    }
    let edges = {
        let state = s.model.state();
        state.body("Round").unwrap().mesh().edges.clone()
    };
    for (x, occurrence) in [(140.0, first), (160.0, second)] {
        let line = line_under(&mut s, sheet, [x, 155.1], 0.5);
        assert_eq!((&line["occurrence"], &line["source"]["kind"]), (&json!([occurrence]), &json!("edge")), "{line}");
        let edge = edges.get(line["source"]["edge"].as_u64().unwrap() as usize).unwrap();
        assert!(edge.iter().all(|p| (p.z - 20.0).abs() < 1e-9), "the top rim: {line}");
    }
}

/// A view's pictures come before its strokes in its group: with `Over` as a shaded reference part over
/// `Under`'s visible lines, a press on `Under`'s top edge (z 10, at y 152.5) still names `Under`, and
/// the same edge as with `Over` left out, which has no picture.
#[test]
fn a_press_lines_up_with_the_strokes_behind_a_views_pictures() {
    let (design, over) = under_and_over();
    let (mut s, sheet, [front, _]) = front_and_top_of(design);
    let drawing = json!({"drawing": ids::encode(s.drawing("test").unwrap().id)});
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "occurrence.bom_type", json!({"occurrence": over, "bom_type": "reference"}));
    run(&mut s, "drawing.open", drawing);
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(front), "set": {"reference_style": "shaded", "reference_shown_percent": 100}}));
    let scene = built(&s, sheet).scene;
    let items = &scene.groups.iter().find(|g| g.object == front).unwrap().items;
    assert!(matches!(items.first(), Some(Item::Picture(_))), "the shaded part's picture comes first");
    let line = line_under(&mut s, sheet, [148.0, 152.6], 0.5);
    assert_eq!((&line["body"], &line["reference"]), (&json!("BodyUnder"), &json!(false)), "{line}");
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(front), "set": {"hide": [[over]]}}));
    let alone = line_under(&mut s, sheet, [148.0, 152.6], 0.5);
    assert_eq!((&alone["body"], &alone["source"]), (&line["body"], &line["source"]), "{line} {alone}");
}

/// A view the sheet leaves out, its lines past a paper scene's records, has no line to press, though
/// its own lines run under the point: the page doesn't show them.
#[test]
fn a_press_on_a_view_the_sheet_leaves_out_names_no_line() {
    let (mut s, sheet, [front, _]) = front_and_top();
    assert_eq!(line_under(&mut s, sheet, [150.0, 155.2], 0.5)["view"], json!(ids::encode(front)));
    // Too many records, with the place, frame and owners the view was drawn with.
    let real = overfill(&mut s, front);
    let drawn = &mut s.drawing_scenes.views.get_mut(&front).unwrap().drawn;
    (drawn.boundary, drawn.frame, drawn.owners) = (real.boundary, real.frame, real.owners);
    assert_eq!(line_under(&mut s, sheet, [150.0, 150.0], 0.5), Value::Null);
    let status = extract(&mut s, sheet).unwrap().views.into_iter().find(|v| v.view == front).map(|v| v.status);
    assert!(matches!(status, Some(Status::Failed(_))), "{status:?}");
}

/// In every style, a view's boundary among the scene's objects is the one inspect says, reference
/// parts and percent included, whether or not the view can be drawn yet (E7a).
#[test]
fn a_views_boundary_in_the_scene_is_its_boundary_in_every_style() {
    let (design, over) = under_and_over();
    let (mut s, sheet, [front, _]) = front_and_top_of(design);
    let drawing = json!({"drawing": ids::encode(s.drawing("test").unwrap().id)});
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "occurrence.bom_type", json!({"occurrence": over, "bom_type": "reference"}));
    run(&mut s, "drawing.open", drawing);
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(front), "set": {"reference_shown_percent": 50}}));
    for style in ["visible", "visible_hidden", "shaded", "shaded_visible", "shaded_visible_hidden"] {
        run(&mut s, "drawing.view.edit", json!({"view": ids::encode(front), "set": {"visual_style": style}}));
        let inspected = run(&mut s, "drawing.view.inspect", json!({"view": ids::encode(front)}));
        let corner = |at: &str| Vec2::new(inspected["boundary_mm"][at][0].as_f64().unwrap(), inspected["boundary_mm"][at][1].as_f64().unwrap());
        let objects = built(&s, sheet).objects;
        let boundary = objects.iter().find(|o| o.object == front).and_then(|o| o.boundary_mm).unwrap();
        assert!((boundary.min - corner("min")).len() < 1e-9 && (boundary.max - corner("max")).len() < 1e-9, "{style}: {boundary:?} {inspected}");
    }
}

/// What a dropped result must leave as it was: the provider's scene and its number, the design,
/// its undo and redo steps, the session's revision and whether it needs saving.
fn untouched(s: &mut Session, sheet: u64) -> (String, u64, String, [usize; 2], u64, bool) {
    let shown = extract(s, sheet).unwrap();
    (bytes(&shown.scene), shown.generation, s.doc.to_json(), [s.undo.len(), s.redo.len()], s.revision, s.is_dirty())
}

/// The block with a 10 mm cube on it, in a component of its own, and [`front_and_top`]'s drawing of
/// them, open: the session, the sheet and the cube's occurrence.
fn cube_on_block() -> (Session, u64, Value) {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 100, "width": 50, "height": 20, "body_name": "Block"}));
    let cube = run(&mut s, "component.create", json!({"name": "Cube", "grounded": false}))["occurrence"].clone();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [0, 0, 20]}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let (s, sheet, _) = front_and_top_of(s);
    (s, sheet, cube)
}

/// Runs model command `id` with the drawing closed, as the model can only be changed then, and
/// opens the drawing again.
fn in_the_model(s: &mut Session, id: &str, p: Value) {
    let drawing = ids::encode(s.open_drawing.unwrap());
    run(s, "drawing.close", json!({}));
    run(s, id, p);
    run(s, "drawing.open", json!({"drawing": drawing}));
}

/// Lines worked out beside the window (#4892 B7) on another thread are taken once they come back,
/// a view at a time, until the provider's scene is the built one, byte for byte; then nothing is
/// asked for. Taking them changes nothing authored and adds no undo step.
#[test]
fn a_job_beside_the_window_draws_as_a_step_does() {
    let (mut s, sheet, _) = front_and_top();
    extract(&mut s, sheet).unwrap();
    let (doc, undo, revision) = (s.doc.to_json(), s.undo.len(), s.revision);
    while let Some(job) = begin(&mut s) {
        let done = std::thread::spawn(move || job.run()).join().unwrap();
        assert!(finish(&mut s, done));
    }
    let end = extract(&mut s, sheet).unwrap();
    assert_eq!((end.stale.len(), bytes(&end.scene), s.drawing_scenes.projections), (0, bytes(&built(&s, sheet).scene), 2));
    assert_eq!((s.doc.to_json(), s.undo.len(), s.revision), (doc, undo, revision));
}

/// Only the latest request's lines are taken: asking again leaves the earlier result to be
/// dropped, whichever comes back first.
#[test]
fn only_the_latest_request_is_taken() {
    for latest_first in [false, true] {
        let (mut s, sheet, _) = front_and_top();
        extract(&mut s, sheet).unwrap();
        let earlier = begin(&mut s).unwrap().run();
        let latest = begin(&mut s).unwrap().run();
        let was = untouched(&mut s, sheet);
        if latest_first {
            assert!(finish(&mut s, latest));
            assert!(!finish(&mut s, earlier));
        } else {
            assert!(!finish(&mut s, earlier));
            assert_eq!(untouched(&mut s, sheet), was);
            assert!(finish(&mut s, latest));
        }
        assert_eq!((extract(&mut s, sheet).unwrap().stale.len(), s.drawing_scenes.projections), (1, 1));
    }
}

/// @0xlillith's four orders at the point a job's lines come back (#4892 W14): after an edit of the
/// drawing or of the model, after an edit and its undo (the view as it was, the design not), after
/// the drawing is closed and opened again, and after a cancel, they are dropped. Dropped, they
/// change nothing on the sheet or in the design, and the next request draws the view as it is now.
#[test]
fn a_job_is_dropped_after_an_edit_an_undo_a_reopen_or_a_cancel() {
    let scale = |s: &mut Session, set: &str| {
        let front = s.drawing("test").unwrap().views[0].id;
        drop(run(s, "drawing.view.edit", json!({"view": ids::encode(front), "set": {"scale": set}})));
    };
    let reopen = |s: &mut Session| {
        let drawing = ids::encode(s.open_drawing.unwrap());
        run(s, "drawing.close", json!({}));
        drop(run(s, "drawing.open", json!({"drawing": drawing})));
    };
    // Each starts from the block with a tower added since `checkpoint`.
    let orders: [(&str, &dyn Fn(&mut Session, &Value)); 5] = [
        ("an edit of the drawing", &|s, _| scale(s, "1:5")),
        ("an edit of the model", &|s, checkpoint| drop(run(s, "edit.restore_checkpoint", json!({"checkpoint": checkpoint})))),
        ("an edit and its undo", &|s, _| {
            scale(s, "1:5");
            drop(run(s, "edit.undo", json!({})));
        }),
        ("closing and opening the drawing", &|s, _| reopen(s)),
        ("a cancel", &|s, _| cancel(s)),
    ];
    for (order, after) in orders {
        let (mut s, sheet, _) = front_and_top();
        let checkpoint = run(&mut s, "edit.checkpoint", json!({}))["checkpoint"].clone();
        let drawing = ids::encode(s.open_drawing.unwrap());
        run(&mut s, "drawing.close", json!({}));
        run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 40, "corner": [40, 60, 0], "body_name": "Tower", "operation": "new"}));
        run(&mut s, "drawing.open", json!({"drawing": drawing}));
        settled(&mut s, sheet);
        scale(&mut s, "1:1");
        let done = begin(&mut s).unwrap().run();
        after(&mut s, &checkpoint);
        let was = untouched(&mut s, sheet);
        assert!(!finish(&mut s, done), "{order}");
        assert_eq!(untouched(&mut s, sheet), was, "{order}");
        assert_eq!(bytes(&settled(&mut s, sheet).scene), bytes(&built(&s, sheet).scene), "{order}");
    }
}

/// A result is dropped too when what it was asked from changes without the session's revision
/// moving: the view as it resolves now (a drawing put in place whole), or the model's world state
/// (a move not captured yet, cleared directly).
#[test]
fn a_job_is_dropped_when_its_view_or_the_model_changed_unannounced() {
    let (mut s, sheet, [front, _]) = front_and_top();
    extract(&mut s, sheet).unwrap();
    let done = begin(&mut s).unwrap().run();
    let mut d = (**s.drawing("test").unwrap()).clone();
    let at = d.views.iter().position(|v| v.id == front).unwrap();
    let mut view = (*d.views[at]).clone();
    view.settings.scale = Some("1:5".parse().unwrap());
    d.views[at] = Arc::new(view);
    s.doc_mut().put_drawing(Arc::new(d));
    let revision = s.revision;
    assert!(!finish(&mut s, done));
    assert_eq!((s.revision, s.drawing_scenes.projections), (revision, 0), "the view");

    let (mut s, sheet, cube) = cube_on_block();
    in_the_model(&mut s, "occurrence.move", json!({"occurrence": cube, "translate": [30, 0, 0], "capture": false}));
    extract(&mut s, sheet).unwrap();
    let done = begin(&mut s).unwrap().run();
    s.pending_moves.clear();
    let revision = s.revision;
    assert!(!finish(&mut s, done));
    assert_eq!((s.revision, s.drawing_scenes.projections), (revision, 0), "the model");
}

/// An occurrence move not captured yet is left out, as dimensions leave it out: a sheet built for
/// an export and the provider's scene both draw the cube on the block where the design places it,
/// as before the move. Once the move is captured, both draw it.
#[test]
fn an_uncaptured_move_is_drawn_where_the_design_places_the_part() {
    let (mut s, sheet, cube) = cube_on_block();
    let placed = settled(&mut s, sheet);
    in_the_model(&mut s, "occurrence.move", json!({"occurrence": cube, "translate": [30, 0, 0], "capture": false}));
    assert_eq!(s.pending_moves.len(), 1);
    assert_eq!(bytes(&built(&s, sheet).scene), bytes(&placed.scene));
    assert_eq!(bytes(&settled(&mut s, sheet).scene), bytes(&placed.scene));
    in_the_model(&mut s, "component.capture_position", json!({}));
    let captured = settled(&mut s, sheet);
    assert_ne!(bytes(&captured.scene), bytes(&placed.scene));
    assert_eq!(bytes(&captured.scene), bytes(&built(&s, sheet).scene));
}

/// A dimension's layout with an occurrence move not captured yet is where the sheet draws it: the
/// block's front bottom edge, with the cube moved 30 up and not captured. The design still places the
/// cube on the block, so the front view's outline is still x 0 to 100 and z 0 to 30, its middle
/// (50, 15) on (150, 150) at 1:2: the edge at y 150 - 15 / 2 = 142.5, from x 150 - 25 = 125 to 175,
/// and its line 10 below, at 132.5, the extension lines from the gap below the edge (141.5) to the
/// overshoot past the line (130.5). Inspect's layout is the one it had before the move, and its tips,
/// extension lines and text's first corner are the built sheet's, exactly. (Moved and captured, the
/// cube would reach z 60 and move the view's middle to z 30.)
#[test]
fn a_dimensions_layout_with_a_move_not_captured_is_where_the_sheet_draws_it() {
    let (mut s, sheet, cube) = cube_on_block();
    let front = s.drawing("test").unwrap().views.iter().find(|v| matches!(v.kind, ViewKind::Base { .. })).unwrap().id;
    let id = front_edge(&mut s, front);
    let layout = |s: &mut Session| run(s, "drawing.dimension.inspect", json!({"dimension": ids::encode(id)}))["layout"].clone();
    let before = layout(&mut s);
    in_the_model(&mut s, "occurrence.move", json!({"occurrence": cube, "translate": [0, 0, 30], "capture": false}));
    assert_eq!(s.pending_moves.len(), 1);
    let now = layout(&mut s);
    assert_eq!(now, before, "as before the move");

    let point = |v: &Value| Vec2::new(v[0].as_f64().unwrap(), v[1].as_f64().unwrap());
    let line = |v: &Value| v.as_array().unwrap().iter().map(point).collect::<Vec<Vec2>>();
    let tips: Vec<Vec2> = now["arrows"].as_array().unwrap().iter().map(|arrow| point(&arrow["tip"])).collect();
    let near = |a: &[Vec2], b: &[Vec2]| a.len() == b.len() && a.iter().zip(b).all(|(p, q)| p.dist(*q) < 1e-9);
    assert!(near(&tips, &[Vec2::new(125.0, 132.5), Vec2::new(175.0, 132.5)]), "{now}");
    // In the order the edge's ends are read, whichever that is.
    let extensions: Vec<Vec<Vec2>> = now["extension_lines"].as_array().unwrap().iter().map(line).collect();
    let left = [Vec2::new(125.0, 141.5), Vec2::new(125.0, 130.5)];
    assert!(extensions.len() == 2 && extensions.iter().any(|extension| near(extension, &left)), "{now}");

    let built = built(&s, sheet);
    let group = group_of(&built.scene, id);
    let (mut strokes, mut drawn_tips, mut origins): (Vec<Vec<Vec2>>, _, _) = (Vec::new(), Vec::new(), Vec::new());
    for item in &group.items {
        match item {
            Item::Stroke { path, .. } => strokes.push(path.iter().map(|seg| seg.start()).chain(path.last().map(|seg| seg.end())).collect()),
            Item::Fill { region } => drawn_tips.push(region.outer.segs[0].start()),
            Item::Text(text) => origins.push(text.origin),
            Item::Picture(_) => {}
        }
    }
    assert_eq!(tips, drawn_tips);
    assert!(extensions.iter().all(|extension| strokes.contains(extension)), "{extensions:?} {strokes:?}");
    let firsts: Vec<Vec2> = now["text_boxes"].as_array().unwrap().iter().map(|corners| point(&corners[0])).collect();
    assert_eq!(firsts, origins);
}

/// Seeded mixes of edits, undos, redos, steps and jobs begun, run, cancelled and taken in any
/// order: once nothing is waiting, the provider's scene is always the built one, byte for byte.
#[test]
fn seeded_edits_steps_and_jobs_always_end_at_the_built_scene() {
    let pick = |seed: u64, salt: u64, n: u64| ((seed ^ salt).wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 33) % n;
    for seed in 1..=24u64 {
        let (mut s, sheet, [front, top]) = front_and_top();
        let edit =
            |s: &mut Session, view: u64, set: Value| s.execute("drawing.view.edit", &json!({"view": ids::encode(view), "set": set})).map(|_| ());
        let mut running = Vec::new();
        for i in 0..16u64 {
            let scale = ["1:1", "1:2", "1:5"][pick(seed, 200 + i, 3) as usize];
            let _ = match pick(seed, i, 9) {
                0 => edit(&mut s, front, json!({"scale": scale})),
                1 => edit(&mut s, top, json!({"place": {"aligned": 200.0 + 10.0 * pick(seed, 100 + i, 5) as f64}})),
                2 => s.execute("edit.undo", &json!({})).map(|_| ()),
                3 => s.execute("edit.redo", &json!({})).map(|_| ()),
                4 => {
                    step(&mut s);
                    Ok(())
                }
                5 => extract(&mut s, sheet).map(drop),
                6 => {
                    running.extend(begin(&mut s).map(Job::run));
                    Ok(())
                }
                7 if !running.is_empty() => {
                    let done = running.remove(pick(seed, 300 + i, running.len() as u64) as usize);
                    finish(&mut s, done);
                    Ok(())
                }
                _ => {
                    cancel(&mut s);
                    Ok(())
                }
            };
        }
        for done in running {
            finish(&mut s, done);
        }
        let end = settled(&mut s, sheet);
        assert!(end.stale.is_empty(), "seed {seed}");
        assert_eq!(bytes(&end.scene), bytes(&built(&s, sheet).scene), "seed {seed}");
    }
}

/// A note on sheet `sheet` at `at`, saying `content`: its id.
fn sheet_note(s: &mut Session, sheet: u64, at: [f64; 2], content: Value) -> u64 {
    let added = run(s, "drawing.note.add", json!({"anchor": {"sheet": ids::encode(sheet)}, "at": at, "content": content}));
    ids::decode(added["note"].as_str().unwrap()).unwrap()
}

/// A note `at` from view `view`'s centre, saying `content`; on its model edge through `on` when given.
fn view_note(s: &mut Session, view: u64, on: Option<[f64; 3]>, at: [f64; 2], content: Value) -> u64 {
    let v = ids::encode(view);
    let anchor = on.map_or_else(|| json!({"view": v}), |point| json!({"edge": {"view": v, "edge": point}}));
    let added = run(s, "drawing.note.add", json!({"anchor": anchor, "at": at, "content": content}));
    ids::decode(added["note"].as_str().unwrap()).unwrap()
}

/// A group's text, run by run, and its arrowheads' tips.
fn text_and_tips(group: &Group) -> (Vec<String>, Vec<Vec2>) {
    let (mut texts, mut tips) = (Vec::new(), Vec::new());
    for item in &group.items {
        match item {
            Item::Text(run) => texts.push(run.text.clone()),
            Item::Fill { region } => tips.push(region.outer.segs[0].start()),
            _ => {}
        }
    }
    (texts, tips)
}

/// "Sheet {number} of {count}": a note's fields, read for its sheet.
fn sheet_of_count() -> Value {
    json!([{"text": "Sheet "}, {"field": {"source": "sheet", "key": "number"}}, {"text": " of "}, {"field": {"source": "sheet", "key": "count"}}])
}

/// Notes are drawn: a view's with its view's dimensions, after its views, and the sheet's after them,
/// each its text, its fields read; one on an edge with its leader's arrowhead on the edge's middle on
/// paper. `build`, the provider and `drawing.scene` agree; `dimensions` lists only dimensions; a point
/// on a note's text hits it.
#[test]
fn notes_are_drawn_on_the_sheet_and_on_their_views() {
    let (mut s, sheet, [front, _]) = front_and_top();
    let dimension = top_edge(&mut s, front);
    let general = sheet_note(&mut s, sheet, [60.0, 40.0], json!("General note"));
    let field = view_note(&mut s, front, None, [0.0, -30.0], sheet_of_count());
    let edge = view_note(&mut s, front, Some([50.0, 0.0, 0.0]), [60.0, -40.0], json!("Deburr"));
    let built = built(&s, sheet);
    let after_views: Vec<(u64, ObjectKind)> = built
        .scene
        .groups
        .iter()
        .map(|g| (g.object, g.kind))
        .skip_while(|(_, k)| matches!(k, ObjectKind::DrawingBorder | ObjectKind::View))
        .collect();
    let mut on_view = vec![(dimension, ObjectKind::Dimension), (field, ObjectKind::Note), (edge, ObjectKind::Note)];
    on_view.sort_by_key(|(id, _)| *id);
    on_view.push((general, ObjectKind::Note));
    assert_eq!(after_views, on_view);
    assert_eq!(text_and_tips(group_of(&built.scene, field)), (vec!["Sheet 1 of 1".to_string()], Vec::new()));
    assert_eq!(text_and_tips(group_of(&built.scene, general)).0, vec!["General note".to_string()]);
    // The front view at 1:2 centred at (150, 150) on the block's middle (50, 10): its bottom edge's
    // middle at (150, 145).
    let (texts, tips) = text_and_tips(group_of(&built.scene, edge));
    assert_eq!(texts, vec!["Deburr".to_string()]);
    assert!(tips.len() == 1 && tips[0].dist(Vec2::new(150.0, 145.0)) < 1e-9, "{tips:?}");
    // Its text centred on its place from the view's centre, (150, 150) + (60, -40): one line, its
    // capitals 3.5 mm high, so its baseline 1.75 mm below the middle.
    let starts: Vec<Vec2> =
        group_of(&built.scene, edge).items.iter().filter_map(|i| if let Item::Text(run) = i { Some(run.origin) } else { None }).collect();
    assert!(starts.len() == 1 && (starts[0].y - 108.25).abs() < 1e-9 && starts[0].x < 210.0, "{starts:?}");
    assert!(built.notes.iter().all(|n| n.drawn && n.status == solidcraft_doc::drawing::dimension::Status::Ok), "{:?}", built.notes);
    assert_eq!(built.notes.last().map(|n| (n.note, n.view)), Some((general, None)));

    let provided = settled(&mut s, sheet);
    assert_eq!((bytes(&provided.scene), &provided.notes), (bytes(&built.scene), &built.notes));
    let reply = run(&mut s, "drawing.scene", json!({"sheet": ids::encode(sheet)}));
    let schema = output_schema_for::<SheetSceneResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &reply), "{reply}");
    assert_eq!(reply["dimensions"].as_array().map(Vec::len), Some(1), "{reply}");
    let notes: Vec<(Value, Option<Value>)> = reply["notes"].as_array().unwrap().iter().map(|n| (n["note"].clone(), n.get("view").cloned())).collect();
    assert_eq!(notes.len(), 3);
    assert!(notes.contains(&(json!(ids::encode(general)), None)) && notes.contains(&(json!(ids::encode(edge)), Some(json!(ids::encode(front))))));
    let export = run(&mut s, "drawing.export", json!({"sheet": ids::encode(sheet)}));
    assert_eq!(export["notes"], reply["notes"]);
    let hit = run(&mut s, "drawing.scene.hit", json!({"sheet": ids::encode(sheet), "point_mm": [60.0, 40.0]}));
    assert_eq!((&hit["hit"]["object"], &hit["hit"]["kind"]), (&json!(ids::encode(general)), &json!("note")), "{hit}");
}

/// Moving a view moves its notes, their same lines moved to its new centre, and projects nothing; the sheet's note stays.
/// A second sheet changes the count both notes read, though neither note nor the view changed.
#[test]
fn a_views_notes_move_with_it_and_read_their_fields_again() {
    let (mut s, sheet, [front, _]) = front_and_top();
    let general = sheet_note(&mut s, sheet, [60.0, 40.0], sheet_of_count());
    let field = view_note(&mut s, front, None, [0.0, -30.0], sheet_of_count());
    let edge = view_note(&mut s, front, Some([50.0, 0.0, 0.0]), [60.0, -40.0], json!("Deburr"));
    let before = settled(&mut s, sheet);
    // Each view note is kept centred on its view, and the scene moves it to the view's centre.
    let kept = |s: &Session, id: u64| s.drawing_scenes.dimensions[&id].drawn.group.clone().unwrap();
    let was = [field, edge].map(|id| kept(&s, id));
    let projections = s.drawing_scenes.projections;
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(front), "set": {"place": {"at": [170.0, 130.0]}}}));
    let after = settled(&mut s, sheet);
    assert_eq!(s.drawing_scenes.projections, projections, "a move projects nothing");
    for (id, was) in [field, edge].into_iter().zip(was) {
        assert_eq!(bytes_of(&kept(&s, id)), bytes_of(&was), "{id}: the same lines");
        assert_eq!(bytes_of(group_of(&after.scene, id)), bytes_of(&moved(&was, Vec2::new(170.0, 130.0))), "{id}: moved to the view's new centre");
    }
    // Where the edge note's arrow ends on the sheet, worked out by hand: the edge's middle (50, 0, 0) at 1:2,
    // from the block's centre (50, 25, 10), is 5 below the view's new centre (170, 130).
    let tip = text_and_tips(group_of(&after.scene, edge)).1[0];
    assert!((tip.x - 170.0).abs() < 1e-9 && (tip.y - 125.0).abs() < 1e-9, "{tip:?}");
    assert_eq!(bytes_of(group_of(&after.scene, general)), bytes_of(group_of(&before.scene, general)));

    run(&mut s, "drawing.sheet.add", json!({}));
    let counted = settled(&mut s, sheet);
    for id in [field, general] {
        assert_eq!(text_and_tips(group_of(&counted.scene, id)).0, vec!["Sheet 1 of 2".to_string()], "{id}");
    }
    assert_eq!(bytes(&counted.scene), bytes(&built(&s, sheet).scene));
}

/// A note on a view with branches draws a leader to each, its first branch as its first leader. A
/// branch whose edge is lost draws nothing, and the note stays drawn and ok, its other leader kept.
#[test]
fn a_notes_branches_are_drawn_and_a_lost_one_draws_nothing() {
    let (mut s, sheet, front, _, drawing) = holed();
    let branches = json!([{"edge": [56, 0, 10]}, {"edge": [50, 0, 0]}]);
    let added =
        run(&mut s, "drawing.note.add", json!({"anchor": {"view": ids::encode(front)}, "at": [60.0, 40.0], "content": "Ream", "branches": branches}));
    let id = ids::decode(added["note"].as_str().unwrap()).unwrap();
    let before = settled(&mut s, sheet);
    let tips = text_and_tips(group_of(&before.scene, id)).1;
    // The bottom edge's middle (50, 0, 0) at 1:2 from the block's middle (50, 10) round (150, 150).
    assert!(tips.len() == 2 && tips[1].dist(Vec2::new(150.0, 145.0)) < 1e-9, "{tips:?}");
    delete_hole(&mut s, &drawing);
    let lost = settled(&mut s, sheet);
    let state = lost.notes.iter().find(|n| n.note == id).unwrap();
    assert!(state.drawn && state.status == solidcraft_doc::drawing::dimension::Status::Ok, "{state:?}");
    let tips = text_and_tips(group_of(&lost.scene, id)).1;
    assert!(tips.len() == 1 && tips[0].dist(Vec2::new(150.0, 145.0)) < 1e-9, "{tips:?}");
    assert_eq!(bytes(&lost.scene), bytes(&built(&s, sheet).scene));
}

/// A note whose edge is lost keeps its text where it was, with no leader, drawn and listed lost;
/// the rest of the sheet is drawn. Undone, its leader is back, the same bytes.
#[test]
fn a_note_whose_edge_is_lost_keeps_its_text() {
    let (mut s, sheet, front, _, drawing) = holed();
    let id = view_note(&mut s, front, Some([56.0, 0.0, 10.0]), [60.0, 40.0], json!("Ream"));
    let before = settled(&mut s, sheet);
    assert_eq!(text_and_tips(group_of(&before.scene, id)).1.len(), 1);
    delete_hole(&mut s, &drawing);
    let lost = settled(&mut s, sheet);
    let state = lost.notes.iter().find(|n| n.note == id).unwrap();
    assert!(state.drawn && matches!(state.status, solidcraft_doc::drawing::dimension::Status::Lost(..)), "{state:?}");
    let (texts, tips) = text_and_tips(group_of(&lost.scene, id));
    assert_eq!((texts, tips.len()), (vec!["Ream".to_string()], 0));
    assert_eq!(bytes(&lost.scene), bytes(&built(&s, sheet).scene));
    let reply = run(&mut s, "drawing.scene", json!({"sheet": ids::encode(sheet)}));
    let listed = reply["notes"].as_array().unwrap().iter().find(|n| n["note"] == json!(ids::encode(id))).cloned().unwrap();
    assert_eq!((&listed["status"], &listed["drawn"]), (&json!("lost"), &json!(true)), "{listed}");

    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "drawing.open", json!({"drawing": drawing}));
    assert_eq!(bytes_of(group_of(&settled(&mut s, sheet).scene, id)), bytes_of(group_of(&before.scene, id)));
}

/// A held deferred drawing keeps a view's note as it keeps its dimensions: closed during a change and
/// opened again, it shows the note as it was and lists it out of date, and a background export
/// writes the same; updated, the note is read again.
#[test]
fn a_held_drawing_keeps_its_notes_and_exports_them_from_a_scratch_copy() {
    let (mut s, sheet, [front, _]) = front_and_top();
    let id = view_note(&mut s, front, Some([50.0, 0.0, 20.0]), [60.0, 40.0], json!("Top"));
    let drawing = json!({"drawing": ids::encode(s.open_drawing.unwrap())});
    run(&mut s, "drawing.set", json!({"updates": "deferred"}));
    run(&mut s, "drawing.update", json!({}));
    let before = settled(&mut s, sheet);
    let open = s.open_drawing.unwrap();
    run(&mut s, "drawing.close", json!({}));
    assert!(s.drawing_scenes.kept.of(open).is_some_and(|kept| kept.dimensions.contains_key(&id)), "kept at the close");
    let block = s.doc.features[0].id;
    run(&mut s, "timeline.edit", json!({"feature": block.to_string(), "set": {"height": 40}}));
    run(&mut s, "drawing.open", drawing);

    let held = settled(&mut s, sheet);
    assert_eq!(bytes_of(group_of(&held.scene, id)), bytes_of(group_of(&before.scene, id)), "as it was");
    assert!(listed(&mut s, sheet).contains(&json!(ids::encode(id))));
    let svg = json!({"sheet": ids::encode(sheet), "format": "svg"});
    let there = s.scratch_as_shown().execute("drawing.export", &svg).unwrap();
    let here = run(&mut s, "drawing.export", svg);
    assert_eq!((&there["svg"], &there["out_of_date"], &there["notes"]), (&here["svg"], &here["out_of_date"], &here["notes"]));

    run(&mut s, "drawing.update", json!({}));
    let now = settled(&mut s, sheet);
    assert_eq!(bytes(&now.scene), bytes(&built(&s, sheet).scene));
    assert_ne!(bytes_of(group_of(&now.scene, id)), bytes_of(group_of(&before.scene, id)), "its leader to the edge 40 tall");
    assert!(!listed(&mut s, sheet).contains(&json!(ids::encode(id))));
}

/// A held deferred drawing lists a note out of date on a view that still shows the same: after a
/// sketch (a new world state that adds no body), the top view takes the new state, up to date, but
/// its note was read from the old one. Updated, the note is read again.
#[test]
fn a_held_drawing_lists_a_note_whose_view_shows_the_same() {
    let (mut s, sheet, [_, top]) = front_and_top();
    let id = view_note(&mut s, top, None, [0.0, -30.0], json!("Top"));
    run(&mut s, "drawing.set", json!({"updates": "deferred"}));
    run(&mut s, "drawing.update", json!({}));
    settled(&mut s, sheet);
    assert!(!listed(&mut s, sheet).contains(&json!(ids::encode(id))));
    // Closed during the change and opened again, as a held drawing is (a drawing open refuses a model change).
    let drawing = json!({"drawing": ids::encode(s.open_drawing.unwrap())});
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Note"}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "drawing.open", drawing);
    assert!(!extract(&mut s, sheet).unwrap().stale.contains(&top), "the top view shows the same");
    assert!(listed(&mut s, sheet).contains(&json!(ids::encode(id))), "held, the note is out of date");

    run(&mut s, "drawing.update", json!({}));
    settled(&mut s, sheet);
    assert!(!listed(&mut s, sheet).contains(&json!(ids::encode(id))), "updated, the note is read again");
}

/// A sheet's note edited is drawn with its new text: the scene kept for the sheet isn't shown again.
#[test]
fn a_sheet_note_edited_draws_its_new_text() {
    let (mut s, sheet, _) = front_and_top();
    let id = sheet_note(&mut s, sheet, [60.0, 40.0], json!("General note"));
    settled(&mut s, sheet);
    run(&mut s, "drawing.note.edit", json!({"note": ids::encode(id), "set": {"content": "Revised note"}}));
    let after = settled(&mut s, sheet);
    assert_eq!(text_and_tips(group_of(&after.scene, id)).0, vec!["Revised note".to_string()]);
    assert_eq!(bytes(&after.scene), bytes(&built(&s, sheet).scene));
}

/// Each body the views of a sheet draw comes with the world state of the session its view drew from
/// (#13054 on #13048): the two assembled views' with the design's, the exploded one's with its copy's.
/// `drawing.check` reads what the sheet draws from these.
#[test]
fn each_drawn_body_comes_with_the_state_its_view_drew_from() {
    let (s, sheet, ..) = two_assembled_one_apart();
    let placed = s.world_state().unwrap();
    let from_design: Vec<bool> = built(&s, sheet).bodies_drawn().map(|(state, _)| Arc::ptr_eq(state, &placed)).collect();
    // Two cubes in each of the three views.
    assert_eq!((from_design.len(), from_design.iter().filter(|design| **design).count()), (6, 4));
}

/// A view placed apart from its projection place has its label above it (#13044): a group of its
/// own kind, painted with the views' annotations, its box over the view's boundary and centred on it,
/// and its state on the view. Moved, it sits at its offset from the view's centre and goes where the
/// view goes; hidden, it isn't drawn. `build` and the provider give the same bytes.
#[test]
fn a_views_label_sits_above_it_and_moves_with_it() {
    let (mut s, sheet, [front, top]) = front_and_top();
    let edit = |s: &mut Session, set: Value| run(s, "drawing.view.edit", json!({"view": ids::encode(top), "set": set}));
    edit(&mut s, json!({"place": {"at": [300.0, 220.0]}}));
    let label_box = |shown: &SheetScene| shown.objects.iter().find(|o| o.kind == ObjectKind::ViewLabel).map(|o| (o.object, o.bounds_mm));
    let shown = built(&s, sheet);
    let (object, bounds) = label_box(&shown).unwrap();
    let boundary = shown.objects.iter().find(|o| o.kind == ObjectKind::View && o.object == top).and_then(|o| o.boundary_mm).unwrap();
    assert_eq!(object, top);
    assert!(bounds.min.y > boundary.max.y && (bounds.center().x - boundary.center().x).abs() < 1.0, "{bounds:?} over {boundary:?}");
    let kinds: Vec<ObjectKind> = shown.scene.groups.iter().map(|g| g.kind).collect();
    assert_eq!(kinds.last(), Some(&ObjectKind::ViewLabel), "after the views");
    let state = |shown: &SheetScene, id: u64| shown.views.iter().find(|v| v.view == id).unwrap().label.clone();
    assert_eq!((state(&shown, top), state(&shown, front)), (Some(LabelState { drawn: true, why: None }), None));
    assert_eq!(bytes(&settled(&mut s, sheet).scene), bytes(&shown.scene));
    edit(&mut s, json!({"label": {"offset": [0.0, -40.0]}}));
    assert!(label_box(&built(&s, sheet)).unwrap().1.center().dist(Vec2::new(300.0, 180.0)) < 1.0);
    edit(&mut s, json!({"place": {"at": [250.0, 220.0]}}));
    assert!(label_box(&built(&s, sheet)).unwrap().1.center().dist(Vec2::new(250.0, 180.0)) < 1.0, "with its view");
    edit(&mut s, json!({"label": {"shown": false}}));
    let hidden = built(&s, sheet);
    assert_eq!((label_box(&hidden), state(&hidden, top)), (None, None));
}

fn cached_sketch_group(scene: &Scene, id: u64) -> &Group {
    scene.groups.iter().find(|group| group.kind == ObjectKind::Sketch && group.object == id).unwrap()
}

#[test]
fn sheet_sketch_provider_extract_is_read_only_and_steps_one_sketch_at_a_time() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane":"XY"}));
    let model_sketch = s.active_sketch.unwrap();
    let line = run(&mut s, "sketch.line", json!({"points":[[0,0],[40,0]]}))["curves"][0].as_str().unwrap().to_string();
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.box", json!({"length":5,"width":5,"height":5}));
    run(&mut s, "sketch.edit", json!({"sketch":model_sketch}));
    run(&mut s, "sketch.drag", json!({"entity":format!("{line}.end"),"from":[40,0],"to":[45,3],"live":true}));
    assert!(s.model.is_stale(), "the source model has an unfinished live gesture");
    let drawing = run(&mut s, "drawing.create", json!({}));
    let first = run(&mut s, "drawing.sketch.create", json!({"owner":drawing["sheets"][0]["id"]}));
    let first = ids::decode(first["sketch"].as_str().unwrap()).unwrap();
    let sheet = s.drawing("test").unwrap().sheets[0].id;
    run(&mut s, "sketch.line", json!({"points":[[15,10],[55,10]],"infer":false}));
    run(&mut s, "sketch.finish", json!({}));
    let second = run(&mut s, "drawing.sketch.create", json!({"owner":ids::encode(sheet)}));
    let second = ids::decode(second["sketch"].as_str().unwrap()).unwrap();
    run(&mut s, "sketch.line", json!({"points":[[20,30],[60,30]],"infer":false}));
    run(&mut s, "sketch.finish", json!({}));
    let doc = s.doc.clone();
    let model = s.model.state();
    let history = (s.undo.len(), s.redo.len(), s.evaluations, s.revision);
    let mut order = vec![first, second];
    order.sort_unstable();
    let initial = extract(&mut s, sheet).unwrap();
    assert_eq!(initial.stale_sketches, order);
    assert!(initial.sketches.is_empty() && initial.scene.groups.iter().all(|group| group.kind != ObjectKind::Sketch));
    assert_eq!((s.drawing_scenes.sketch_solves, s.drawing_scenes.projections), (0, 0));
    assert!(step(&mut s));
    let one = extract(&mut s, sheet).unwrap();
    assert_eq!(one.stale_sketches, [order[1]]);
    assert_eq!(one.sketches.iter().map(|state| state.sketch).collect::<Vec<_>>(), [order[0]]);
    assert_eq!(s.drawing_scenes.sketch_solves, 1);
    assert!(!step(&mut s));
    let complete = extract(&mut s, sheet).unwrap();
    assert!(complete.stale_sketches.is_empty() && complete.stale.is_empty());
    assert_eq!(
        complete.sketches.iter().map(|state| (state.sketch, state.status.name())).collect::<Vec<_>>(),
        order.iter().map(|id| (*id, "drawn")).collect::<Vec<_>>()
    );
    assert_eq!(bytes(&complete.scene), bytes(&built(&s, sheet).scene));
    let again = extract(&mut s, sheet).unwrap();
    assert!(Arc::ptr_eq(&again.scene, &complete.scene) && again.generation == complete.generation);
    assert!(!step(&mut s));
    assert_eq!((s.drawing_scenes.sketch_solves, s.drawing_scenes.projections), (2, 0));
    assert!(Arc::ptr_eq(&doc, &s.doc) && Arc::ptr_eq(&model, &s.model.state()));
    assert_eq!((s.undo.len(), s.redo.len(), s.evaluations, s.revision), history);
    assert!(s.model.is_stale() && s.active_sketch == Some(model_sketch));
}

#[test]
fn sheet_sketch_provider_source_key_reads_parameters_feature_inputs_and_units_without_model_evaluation() {
    let (mut s, drawing, id) = stroke_editor();
    let sheet = s.drawing("test").unwrap().sheets[0].id;
    // This unitless ratio depends on document length defaults while its ParamDef stays the same:
    // (2 inches + 1 mm) / 1 mm = 51.8; in a millimetre design it is 3.
    s.doc_mut().set_param("Ratio", "(2 + 1 mm) / 1 mm", Some(""), None).unwrap();
    let feature = s.doc.features.iter().find(|feature| !feature.param_names.is_empty()).unwrap();
    let feature_id = feature.id;
    let feature_param = feature.param_names[0].clone();
    assert!(s.doc.params.iter().all(|param| param.name != feature_param));
    run(&mut s, "sketch.line", json!({"points":[["Inset + 5","10"],["Inset + 20","10"]],"infer":false}));
    run(&mut s, "sketch.line", json!({"points":[[format!("{feature_param} + 5"),"20"],[format!("{feature_param} + 20"),"20"]],"infer":false}));
    run(&mut s, "sketch.line", json!({"points":[["Ratio * 1 mm","30"],["Ratio * 1 mm + 10 mm","30"]],"infer":false}));
    run(&mut s, "sketch.finish", json!({}));
    let record = s.drawing("test").unwrap().sheet_sketch(id).unwrap().clone();
    let owner = s.drawing("test").unwrap().clone();
    let model = s.model.state();
    let history = (s.undo.len(), s.redo.len(), s.evaluations, s.revision);
    let was_stale = s.model.is_stale();
    let positions = |scene: &Scene| cached_sketch_group(scene, id).items.iter().map(|item| stroke_path(item).0[0].start().x).collect::<Vec<_>>();
    let mut previous = settled(&mut s, sheet);
    assert_eq!(positions(&previous.scene), [30.4, 45.0, 51.8]);
    for stage in 0..3 {
        match stage {
            0 => s.doc_mut().set_param("Inset", "2 in", Some("in"), None).unwrap(),
            1 => {
                let params = s.doc.params.clone();
                let feature = s.doc_mut().features.iter_mut().find(|feature| feature.id == feature_id).unwrap();
                let (_, expression, _) = feature.kind.inputs_mut().into_iter().next().unwrap();
                *expression = "60".into();
                assert_eq!(s.doc.params, params, "named feature inputs are not stored in doc.params");
            }
            _ => {
                let defs = s.doc.param_defs();
                s.doc_mut().units = "mm".into();
                assert_eq!(s.doc.param_defs(), defs, "this change needs the separate document-unit key");
            }
        }
        assert!(Arc::ptr_eq(s.doc.drawing(drawing).unwrap(), &owner));
        assert!(Arc::ptr_eq(s.doc.drawing(drawing).unwrap().sheet_sketch(id).unwrap(), &record));
        let doc = s.doc.clone();
        let count = s.drawing_scenes.sketch_solves;
        let pending = extract(&mut s, sheet).unwrap();
        assert_eq!(pending.stale_sketches, [id]);
        assert!(Arc::ptr_eq(&pending.scene, &previous.scene) && pending.generation == previous.generation);
        assert_eq!(s.drawing_scenes.sketch_solves, count, "extract only records stale work");
        assert!(!step(&mut s));
        previous = extract(&mut s, sheet).unwrap();
        assert!(previous.stale_sketches.is_empty());
        assert_eq!(s.drawing_scenes.sketch_solves, count + 1);
        assert_eq!(bytes(&previous.scene), bytes(&built(&s, sheet).scene));
        let expected = match stage {
            0 => [55.8, 45.0, 51.8],
            1 => [55.8, 65.0, 51.8],
            _ => [55.8, 65.0, 3.0],
        };
        for (actual, expected) in positions(&previous.scene).iter().zip(expected) {
            assert!((actual - expected).abs() < 1e-9, "stage {stage}: {actual} != {expected}");
        }
        assert!(Arc::ptr_eq(&doc, &s.doc) && Arc::ptr_eq(&model, &s.model.state()));
        assert_eq!((s.undo.len(), s.redo.len(), s.evaluations, s.revision), history);
        assert_eq!(s.drawing_scenes.projections, 0);
        assert_eq!(s.model.is_stale(), was_stale, "paper resolution preserves the source model's evaluation state");
    }
}

#[test]
fn sheet_sketch_provider_keeps_mixed_paper_order_and_reuses_view_moves_and_editor_state() {
    let mut design = Session::default();
    run(&mut design, "solid.box", json!({"length":100,"width":50,"height":20,"body_name":"Block"}));
    run(&mut design, "document.units", json!({"units":"in"}));
    let (mut s, sheet, [front, top]) = front_and_top_of(design);
    run(&mut s, "drawing.view.edit", json!({"view":ids::encode(top),"set":{"scale":"1:4"}}));
    let made = run(&mut s, "drawing.sketch.create", json!({"owner":ids::encode(sheet)}));
    let id = ids::decode(made["sketch"].as_str().unwrap()).unwrap();
    run(&mut s, "sketch.rectangle.two_point", json!({"p0":[15,10],"p1":[55,30]}));
    run(&mut s, "sketch.finish", json!({}));
    let before = settled(&mut s, sheet);
    let expected = built(&s, sheet);
    assert_eq!(bytes(&before.scene), bytes(&expected.scene));
    let mut views = [front, top];
    views.sort_unstable();
    assert_eq!(
        before.scene.groups.iter().skip(3).map(|group| (group.object, group.kind)).collect::<Vec<_>>(),
        [(views[0], ObjectKind::View), (views[1], ObjectKind::View), (id, ObjectKind::Sketch)]
    );
    let (lo, hi, perimeter) = reach(cached_sketch_group(&before.scene, id));
    assert!(lo.dist(Vec2::new(15.0, 10.0)) < 1e-9 && hi.dist(Vec2::new(55.0, 30.0)) < 1e-9 && (perimeter - 120.0).abs() < 1e-9);
    let counts = (s.drawing_scenes.projections, s.drawing_scenes.sketch_solves);
    let sketch_bytes = serde_json::to_string(cached_sketch_group(&before.scene, id)).unwrap();
    run(&mut s, "drawing.view.edit", json!({"view":ids::encode(front),"set":{"place":{"at":[200,150]}}}));
    let moved = extract(&mut s, sheet).unwrap();
    assert!(moved.stale.is_empty() && moved.stale_sketches.is_empty());
    assert_eq!((s.drawing_scenes.projections, s.drawing_scenes.sketch_solves), counts);
    assert_eq!(serde_json::to_string(cached_sketch_group(&moved.scene, id)).unwrap(), sketch_bytes);
    assert_eq!(bytes(&moved.scene), bytes(&built(&s, sheet).scene));
    run(&mut s, "drawing.sketch.edit", json!({"sketch":ids::encode(id)}));
    run(&mut s, "sketch.finish", json!({}));
    let editing_ended = extract(&mut s, sheet).unwrap();
    assert!(editing_ended.stale_sketches.is_empty() && Arc::ptr_eq(&editing_ended.scene, &moved.scene));
    assert_eq!((s.drawing_scenes.projections, s.drawing_scenes.sketch_solves), counts);
}

#[test]
fn sheet_sketch_provider_reuses_sheets_and_retires_deleted_undone_and_closed_owners() {
    let (mut s, drawing, first) = stroke_editor();
    let first_sheet = s.drawing("test").unwrap().sheets[0].id;
    run(&mut s, "sketch.line", json!({"points":[[10,10],[30,10]],"infer":false}));
    run(&mut s, "sketch.finish", json!({}));
    let second_sheet = run(&mut s, "drawing.sheet.add", json!({"name":"Second"}));
    let second_sheet = ids::decode(second_sheet["sheet"].as_str().unwrap()).unwrap();
    let second = run(&mut s, "drawing.sketch.create", json!({"owner":ids::encode(second_sheet)}));
    let second = ids::decode(second["sketch"].as_str().unwrap()).unwrap();
    run(&mut s, "sketch.line", json!({"points":[[60,60],[90,60]],"infer":false}));
    run(&mut s, "sketch.finish", json!({}));
    let first_scene = settled(&mut s, first_sheet);
    let second_scene = settled(&mut s, second_sheet);
    assert_eq!(s.drawing_scenes.sketch_solves, 2);
    let again = extract(&mut s, first_sheet).unwrap();
    assert!(again.stale_sketches.is_empty());
    assert_eq!(bytes(&again.scene), bytes(&first_scene.scene));
    assert_eq!(s.drawing_scenes.sketch_solves, 2, "switching sheets retains settled drawing entries");
    assert!(again.scene.groups.iter().all(|group| group.kind != ObjectKind::Sketch || group.object == first));
    extract(&mut s, second_sheet).unwrap();
    run(&mut s, "drawing.sheet.delete", json!({"sheet":ids::encode(second_sheet),"contents":"delete"}));
    assert!(!step(&mut s), "a deleted extracted sheet supplies no work");
    assert!(extract(&mut s, second_sheet).is_err());
    let remaining = settled(&mut s, first_sheet);
    assert!(remaining.scene.groups.iter().all(|group| group.object != second));
    assert!(!s.drawing_scenes.sketches.contains_key(&second));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(extract(&mut s, second_sheet).unwrap().stale_sketches, [second]);
    assert_eq!(bytes(&settled(&mut s, second_sheet).scene), bytes(&second_scene.scene));
    run(&mut s, "drawing.close", json!({}));
    assert!(s.drawing_scenes.sketches.is_empty() && s.drawing_scenes.source.is_none() && s.drawing_scenes.context.is_none());
    assert!(!step(&mut s));
    run(&mut s, "drawing.open", json!({"drawing":ids::encode(drawing)}));
    assert_eq!(extract(&mut s, first_sheet).unwrap().stale_sketches, [first]);
    assert_eq!(bytes(&settled(&mut s, first_sheet).scene), bytes(&first_scene.scene));
    let document = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
    let mut reopened = Session::new(document);
    run(&mut reopened, "drawing.open", json!({"drawing":ids::encode(drawing)}));
    assert_eq!(bytes(&settled(&mut reopened, second_sheet).scene), bytes(&second_scene.scene));
}

#[test]
fn sheet_sketch_provider_caches_named_source_failures_and_recovers_without_global_error_blocking() {
    let (mut s, drawing, id) = stroke_editor();
    let sheet = s.drawing("test").unwrap().sheets[0].id;
    run(&mut s, "sketch.line", json!({"points":[["Inset + 5","10"],["Inset + 20","10"]],"infer":false}));
    run(&mut s, "sketch.finish", json!({}));
    s.doc_mut().params.iter_mut().find(|param| param.name == "Inset").unwrap().expr = "Missing + 1 mm".into();
    let doc = s.doc.clone();
    let model = s.model.state();
    let history = (s.evaluations, s.undo.len(), s.redo.len());
    let pending = extract(&mut s, sheet).unwrap();
    assert_eq!(pending.stale_sketches, [id]);
    assert!(pending.sketches.is_empty() && s.drawing_scenes.sketch_solves == 0);
    assert!(!step(&mut s));
    let failed = extract(&mut s, sheet).unwrap();
    assert!(failed.stale_sketches.is_empty());
    assert!(matches!(&failed.sketches[0].status, SketchStatus::Failed(why) if why.contains(&ids::encode(id)) && why.contains("Inset")));
    assert!(failed.scene.groups.iter().all(|group| group.kind != ObjectKind::Sketch));
    for _ in 0..3 {
        assert!(!step(&mut s));
        let cached = extract(&mut s, sheet).unwrap();
        assert!(Arc::ptr_eq(&cached.scene, &failed.scene) && cached.generation == failed.generation);
    }
    assert_eq!(s.drawing_scenes.sketch_solves, 1);
    assert!(Arc::ptr_eq(&doc, &s.doc) && Arc::ptr_eq(&model, &s.model.state()));
    assert_eq!((s.evaluations, s.undo.len(), s.redo.len()), history);
    s.doc_mut().set_param("Inset", "1 in", Some("in"), None).unwrap();
    let restored = settled(&mut s, sheet);
    assert_eq!(restored.sketches[0].status, SketchStatus::Drawn);
    s.doc_mut().params.push(solidcraft_doc::Param {
        name: "Unrelated".into(),
        expr: "NoSuchParameter".into(),
        unit: "mm".into(),
        comment: String::new(),
        model: false,
    });
    assert!(s.doc.param_values().1.contains_key("Unrelated"));
    assert_eq!(settled(&mut s, sheet).sketches[0].status, SketchStatus::Drawn);
    let mut no_source = (**s.drawing("test").unwrap()).clone();
    let sources = std::mem::take(&mut no_source.sources);
    no_source.validate().unwrap();
    s.doc_mut().put_drawing(Arc::new(no_source));
    let failed = settled(&mut s, sheet);
    assert!(
        matches!(&failed.sketches[0].status, SketchStatus::Failed(why) if why.contains("declared design source") && why.contains(&ids::encode(id)))
    );
    let count = s.drawing_scenes.sketch_solves;
    assert!(!step(&mut s));
    assert_eq!(s.drawing_scenes.sketch_solves, count);
    let mut repaired = (**s.doc.drawing(drawing).unwrap()).clone();
    repaired.sources = sources;
    s.doc_mut().put_drawing(Arc::new(repaired));
    assert_eq!(settled(&mut s, sheet).sketches[0].status, SketchStatus::Drawn);
}

#[test]
fn sheet_sketch_provider_invalid_header_failures_follow_repairs_with_the_same_sketch_arc() {
    let (mut s, _, id) = stroke_editor();
    let sheet = s.drawing("test").unwrap().sheets[0].id;
    run(&mut s, "sketch.line", json!({"points":[[10,10],[30,10]],"infer":false}));
    run(&mut s, "sketch.finish", json!({}));
    let record = s.drawing("test").unwrap().sheet_sketch(id).unwrap().clone();
    let before = settled(&mut s, sheet);
    let mut invalid = (**s.drawing("test").unwrap()).clone();
    let name = std::mem::take(&mut invalid.name);
    s.doc_mut().put_drawing(Arc::new(invalid));
    let failed = settled(&mut s, sheet);
    assert!(matches!(&failed.sketches[0].status, SketchStatus::Failed(why) if why.contains(&ids::encode(id))));
    assert!(failed.scene.groups.iter().all(|group| group.kind != ObjectKind::Sketch));
    let count = s.drawing_scenes.sketch_solves;
    assert!(!step(&mut s));
    assert_eq!(s.drawing_scenes.sketch_solves, count);
    let mut repaired = (**s.drawing("test").unwrap()).clone();
    repaired.name = name;
    assert!(Arc::ptr_eq(repaired.sheet_sketch(id).unwrap(), &record));
    s.doc_mut().put_drawing(Arc::new(repaired));
    let after = settled(&mut s, sheet);
    assert_eq!(after.sketches[0].status, SketchStatus::Drawn);
    assert_eq!(bytes(&after.scene), bytes(&before.scene));
    assert_eq!(s.drawing_scenes.sketch_solves, count + 1);
}

#[test]
fn sheet_sketch_assembly_budget_names_only_the_excluded_sketch_and_can_recover() {
    let (s, sheet, _) = front_and_top();
    let d = s.drawing("test").unwrap();
    let record = d.sheets.iter().find(|record| record.id == sheet).unwrap();
    let group = |id, kind, n: usize| Group {
        object: id,
        kind,
        items: vec![Item::Stroke {
            path: (0..n).map(|i| Seg2::Line { a: Vec2::new(i as f64 * 1e-3, 0.0), b: Vec2::new((i + 1) as f64 * 1e-3, 0.0) }).collect(),
            closed: false,
            width_mm: 0.5,
            pattern: None,
        }],
    };
    let left = MAX_RECORDS - border(&s, sheet).iter().map(Group::records).sum::<usize>();
    let view = Drawn {
        hatches: Vec::new(),
        group: Some(Arc::new(group(10, ObjectKind::View, left - 12))),
        status: Status::Drawn,
        stats: None,
        boundary: None,
        frame: None,
        owners: None,
        why: None,
    };
    let sketches = [
        DrawnSketch { group: Some(group(101, ObjectKind::Sketch, 10)), status: SketchStatus::Drawn },
        DrawnSketch { group: Some(group(102, ObjectKind::Sketch, 8)), status: SketchStatus::Drawn },
    ];
    let drawn = || sketches.iter().enumerate().map(|(i, drawn)| (101 + i as u64, drawn));
    let (scene, views, _, states) =
        assemble_with_sketches(record, Projection::Third, &[], std::iter::once((10, Vec2::ZERO, &view)), drawn()).unwrap();
    assert_eq!(views[0].status, Status::Drawn);
    assert!(matches!(&states[0].status, SketchStatus::Failed(why) if why.contains(&ids::encode(101)) && why.contains(&MAX_RECORDS.to_string())));
    assert_eq!(states[1].status, SketchStatus::Drawn);
    assert_eq!(scene.groups.iter().map(Group::records).sum::<usize>(), MAX_RECORDS);
    assert_eq!(scene.groups.iter().skip(3).map(|group| group.object).collect::<Vec<_>>(), [10, 102]);
    let (_, _, _, recovered) = assemble_with_sketches(record, Projection::Third, &[], std::iter::empty::<(u64, Vec2, Drawn)>(), drawn()).unwrap();
    assert!(recovered.iter().all(|state| state.status == SketchStatus::Drawn), "composition budget failures are not permanent geometry failures");
}

#[test]
fn sheet_sketch_strokes_refuse_tiny_arcs_that_shared_pieces_widen() {
    use solidcraft_sketch::CurveKind;
    let (mut s, owner, id) = stroke_editor();
    run(&mut s, "sketch.line", json!({"points":[[10,10],[30,10]],"infer":false}));
    let mut solved = s.doc.solve_sheet_sketch(owner, id).unwrap();
    solved.sketch.curves[0].kind = CurveKind::Arc { c: 0, a: 1, b: 2 };
    for (a, b) in [(Vec2::new(1000.0, 0.0), Vec2::new(1000.0, 5e-10)), (Vec2::new(-1000.0, 2.5e-10), Vec2::new(-1000.0, -2.5e-10))] {
        solved.sketch.points[1].pos = a;
        solved.sketch.points[2].pos = b;
        let why = sketch::paper_group(&solved, 0.5).unwrap_err().to_string();
        assert!(why.contains("sweep") && why.contains(&solved.sketch.curves[0].id), "{why}");
        // The opposite travel is a real almost-full turn, which must remain printable.
        solved.sketch.points[1].pos = b;
        solved.sketch.points[2].pos = a;
        let group = sketch::paper_group(&solved, 0.5).unwrap();
        let (path, closed, _) = stroke_path(&group.items[0]);
        assert!(!closed);
        assert!(matches!(path,[Seg2::Arc{sweep,..}] if *sweep > std::f64::consts::TAU-1e-10 && *sweep <=std::f64::consts::TAU));
    }
    solved.sketch.points[1].pos = Vec2::new(1000.0, 0.0);
    solved.sketch.points[2].pos = solved.sketch.points[1].pos;
    let group = sketch::paper_group(&solved, 0.5).unwrap();
    let (path, closed, _) = stroke_path(&group.items[0]);
    assert!(closed && matches!(path,[Seg2::Arc{sweep,..}] if *sweep == std::f64::consts::TAU));
}

#[test]
fn sheet_sketch_strokes_name_an_arc_past_paper_tolerance_even_when_solved() {
    use solidcraft_sketch::CurveKind;
    let (mut s, owner, id) = stroke_editor();
    run(&mut s, "sketch.arc.center_point", json!({"center":[30,40],"start":[40,40],"sweep":90}));
    let mut drawing = (**s.doc.drawing(owner).unwrap()).clone();
    let record = Arc::make_mut(&mut drawing.sketches[0]);
    let CurveKind::Arc { b, .. } = record.sketch.curves[0].kind else { panic!("not an arc") };
    record.sketch.points[b].pos.y += 5e-8;
    for point in &mut record.sketch.points {
        point.fixed = true;
    }
    record.sketch.curves[0].fixed = true;
    drawing.validate().unwrap();
    s.doc_mut().put_drawing(Arc::new(drawing));
    let before = s.doc.clone();
    let solved = s.doc.solve_sheet_sketch(owner, id).unwrap();
    assert!(solved.report.keeps_orientation());
    assert!(solved.report.max_residual > solidcraft_geom::EPS && solved.report.max_residual < 1e-6, "{:?}", solved.report);
    assert_eq!(solved.sketch.points, before.drawing(owner).unwrap().sheet_sketch(id).unwrap().sketch.points);
    let why = sketch::paper_group(&solved, 0.5).unwrap_err().to_string();
    assert!(why.contains("paper endpoint tolerance") && why.contains(&solved.sketch.curves[0].id), "{why}");
    assert!(Arc::ptr_eq(&before, &s.doc));
}

/// One real sheet keeps upstream dimension frames/status and authored sketch millimetres together.
#[test]
fn dimensions_and_sheet_sketches_share_one_scene_and_one_work_item_per_tick() {
    let (mut s, sheet, [front, _]) = front_and_top();
    let dimension = top_edge(&mut s, front);
    let made = run(&mut s, "drawing.sketch.create", json!({"owner": ids::encode(sheet)}));
    let sketch = ids::decode(made["sketch"].as_str().unwrap()).unwrap();
    run(&mut s, "sketch.line", json!({"points": [[15, 10], [55, 10]], "infer": false}));
    run(&mut s, "sketch.finish", json!({}));
    let doc = s.doc.clone();
    let model = s.model.state();
    let history = (s.undo.len(), s.redo.len(), s.evaluations);
    let first = extract(&mut s, sheet).unwrap();
    assert_eq!(first.stale.len(), 2);
    assert_eq!(first.stale_sketches, vec![sketch]);
    let mut work = Vec::new();
    let provided = loop {
        assert!(work.len() < 4, "two view jobs including their annotations and one sketch suffice");
        let projections = s.drawing_scenes.projections;
        let solves = s.drawing_scenes.sketch_solves;
        let serial = s.drawing_scenes.dimensions.get(&dimension).map(|entry| entry.serial);
        let more = step(&mut s);
        let one = (
            s.drawing_scenes.projections - projections,
            s.drawing_scenes.sketch_solves - solves,
            usize::from(s.drawing_scenes.dimensions.get(&dimension).map(|entry| entry.serial) != serial),
        );
        // A current view job returns its own annotations with its projection.
        // It must never perform the separately scheduled sheet-sketch solve too.
        assert!(matches!(one, (1, 0, 0 | 1) | (0, 0, 1) | (0, 1, 0)), "one job per tick: {one:?}");
        work.push(one);
        let now = extract(&mut s, sheet).unwrap();
        if !more {
            assert!(now.stale.is_empty() && now.stale_dimensions.is_empty() && now.stale_sketches.is_empty());
            break now;
        }
    };
    assert_eq!(work.len(), 3);
    assert_eq!(work[2], (0, 1, 0));
    assert!(work[..2].contains(&(1, 0, 1)) && work[..2].contains(&(1, 0, 0)), "one view job carries the dimension: {work:?}");
    let expected = built(&s, sheet);
    assert_eq!(bytes(&provided.scene), bytes(&expected.scene));
    assert_eq!(provided.dimensions, expected.dimensions);
    assert_eq!(provided.dimensions[0].status, solidcraft_doc::drawing::dimension::Status::Ok);
    assert!(provided.dimensions[0].drawn);
    assert_eq!(provided.sketches[0].status, SketchStatus::Drawn);
    let kinds: Vec<_> = provided.scene.groups.iter().map(|group| group.kind).collect();
    let dimension_at = kinds.iter().position(|kind| *kind == ObjectKind::Dimension).unwrap();
    let sketch_at = kinds.iter().position(|kind| *kind == ObjectKind::Sketch).unwrap();
    assert!(kinds[..dimension_at].contains(&ObjectKind::View) && dimension_at < sketch_at);
    let tips: Vec<_> = group_of(&provided.scene, dimension)
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Fill { region } => Some(region.outer.segs[0].start()),
            _ => None,
        })
        .collect();
    assert_eq!(tips.len(), 2);
    assert!(((tips[0].x - tips[1].x).abs() - 50.0).abs() < 1e-9, "{tips:?}");
    assert!(tips.iter().all(|tip| (tip.y - 165.0).abs() < 1e-9), "{tips:?}");
    let (path, closed, _) = stroke_path(&cached_sketch_group(&provided.scene, sketch).items[0]);
    assert!(!closed);
    assert_eq!(path, &[Seg2::Line { a: Vec2::new(15.0, 10.0), b: Vec2::new(55.0, 10.0) }]);
    let exported = run(&mut s, "drawing.export", json!({"sheet": ids::encode(sheet)}));
    assert_eq!(exported["dimensions"], json!([{"dimension": ids::encode(dimension), "view": ids::encode(front), "status": "ok", "drawn": true}]));
    assert_eq!(exported["sketches"], json!([{"sketch": ids::encode(sketch), "status": "drawn"}]));
    let prepared = provided.scene.prepare().unwrap();
    let (svg, _) = solidcraft_io::paper::paper_svg(&prepared, &solidcraft_geom::paper::NoPictures, solidcraft_doc::drawing::DEFAULT_DPI).unwrap();
    assert_eq!(exported["svg"].as_str().unwrap().as_bytes(), svg);
    assert!(Arc::ptr_eq(&doc, &s.doc) && Arc::ptr_eq(&model, &s.model.state()));
    assert_eq!((s.undo.len(), s.redo.len(), s.evaluations), history);

    let (projections, solves, serial) =
        (s.drawing_scenes.projections, s.drawing_scenes.sketch_solves, s.drawing_scenes.dimensions[&dimension].serial);
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(front), "set": {"place": {"at": [170.0, 130.0]}}}));
    let moved_scene = settled(&mut s, sheet);
    assert_eq!((s.drawing_scenes.projections, s.drawing_scenes.sketch_solves), (projections, solves));
    assert_eq!(s.drawing_scenes.dimensions[&dimension].serial, serial, "a view move does not reread dimensions");
    assert_eq!(bytes_of(group_of(&moved_scene.scene, dimension)), bytes_of(&moved(group_of(&provided.scene, dimension), Vec2::new(20.0, -20.0))));
    assert_eq!(bytes_of(cached_sketch_group(&moved_scene.scene, sketch)), bytes_of(cached_sketch_group(&provided.scene, sketch)));

    run(&mut s, "drawing.dimension.edit", json!({"dimension": ids::encode(dimension), "set": {"offset_mm": 20.0}}));
    let stale = extract(&mut s, sheet).unwrap();
    assert!(stale.stale.is_empty());
    assert_eq!(stale.stale_dimensions, vec![dimension]);
    assert_eq!(stale.stale_sketches, vec![sketch], "the existing complete-Drawing sketch admission key remains conservative");
    assert!(step(&mut s), "the dimension batch leaves the independently stale sketch for another tick");
    assert_eq!((s.drawing_scenes.projections, s.drawing_scenes.sketch_solves), (projections, solves));
    let between = extract(&mut s, sheet).unwrap();
    assert!(between.stale_dimensions.is_empty());
    assert_eq!(between.stale_sketches, vec![sketch]);
    assert!(!step(&mut s));
    assert_eq!((s.drawing_scenes.projections, s.drawing_scenes.sketch_solves), (projections, solves + 1));
    let edited = extract(&mut s, sheet).unwrap();
    assert_eq!(bytes(&edited.scene), bytes(&built(&s, sheet).scene));
    assert_ne!(bytes_of(group_of(&edited.scene, dimension)), bytes_of(group_of(&moved_scene.scene, dimension)));
    assert_eq!(bytes_of(cached_sketch_group(&edited.scene, sketch)), bytes_of(cached_sketch_group(&provided.scene, sketch)));
}

/// Closed deferred drawings keep authored paper marks without retaining the source document.
#[test]
fn held_sheet_sketch_survives_close_reopen_and_scratch_export() {
    let (mut s, drawing, sketch) = stroke_editor();
    let sheet = s.drawing("test").unwrap().sheets[0].id;
    run(&mut s, "sketch.line", json!({"points":[[10,10],[30,10]],"infer":false}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "drawing.set", json!({"updates":"deferred"}));
    run(&mut s, "drawing.update", json!({}));
    let before = settled(&mut s, sheet);
    let marks = bytes_of(cached_sketch_group(&before.scene, sketch));
    run(&mut s, "drawing.close", json!({}));
    assert!(s.drawing_scenes.sketches.is_empty());
    assert!(s.drawing_scenes.source.is_none() && s.drawing_scenes.context.is_none());
    let kept = s.drawing_scenes.kept.of(drawing).unwrap();
    assert_eq!(bytes_of(kept.sketches[&sketch].drawn.group.as_ref().unwrap()), marks);
    run(&mut s, "drawing.open", json!({"drawing":ids::encode(drawing)}));
    let export = json!({"sheet":ids::encode(sheet),"format":"svg"});
    let there = s.scratch_as_shown().execute("drawing.export", &export).unwrap();
    let here = run(&mut s, "drawing.export", export);
    assert_eq!((&there["svg"], &there["out_of_date"]), (&here["svg"], &here["out_of_date"]));
    assert!(here["out_of_date"].as_array().unwrap().contains(&json!(ids::encode(sketch))), "{here}");
    let held = extract(&mut s, sheet).unwrap();
    assert_eq!(bytes_of(cached_sketch_group(&held.scene, sketch)), marks);
    assert_eq!(held.stale_sketches, [sketch]);
    assert!(!step(&mut s), "held means no sketch solve");
    run(&mut s, "drawing.update", json!({}));
    let updated = settled(&mut s, sheet);
    assert!(updated.stale_sketches.is_empty());
    assert_eq!(bytes_of(cached_sketch_group(&updated.scene, sketch)), marks);
}

fn w38d_scene_fixture() -> (Session, u64, u64, u64, Value) {
    let (mut s, drawing, sketch) = stroke_editor();
    let sheet = s.drawing("test").unwrap().sheets[0].id;
    run(&mut s, "sketch.rectangle.two_point", json!({"p0":[15,10],"p1":[55,30]}));
    run(&mut s, "sketch.finish", json!({}));
    let inspected = run(&mut s, "drawing.fill.profiles", json!({"sketch":ids::encode(sketch)}));
    let boundary = inspected["fill_profiles"]["boundaries"][0].clone();
    (s, drawing, sheet, sketch, boundary)
}

fn w38d_create_fill(s: &mut Session, sketch: u64, boundary: &Value, paint: Value) -> u64 {
    let reply = run(s, "drawing.sketch.fill", json!({"sketch":ids::encode(sketch),"boundary":boundary,"paint":paint}));
    ids::decode(reply["fills"][0]["fill"].as_str().unwrap()).unwrap()
}

#[test]
fn sheet_fill_provider_extract_does_no_work_and_multiple_fills_share_one_region_solve() {
    let (mut s, drawing, sheet, sketch, boundary) = w38d_scene_fixture();
    let a = w38d_create_fill(&mut s, sketch, &boundary, json!({"kind":"solid"}));
    let b = w38d_create_fill(&mut s, sketch, &boundary, json!({"kind":"solid"}));
    let mut expected = [a, b];
    expected.sort_unstable();
    let doc = s.doc.clone();
    let model = s.model.state();
    let counters = (s.evaluations, s.undo.len(), s.redo.len(), s.revision);
    let initial = extract(&mut s, sheet).unwrap();
    assert_eq!(initial.stale_fills, expected);
    assert!(initial.fills.is_empty());
    assert_eq!((s.drawing_scenes.fill_solves, s.drawing_scenes.fill_expansions), (0, 0));
    assert!(step(&mut s));
    let partial = extract(&mut s, sheet).unwrap();
    assert_eq!(partial.fills.len(), 1);
    assert_eq!(partial.stale_fills.len(), 1);
    assert_eq!((s.drawing_scenes.fill_solves, s.drawing_scenes.fill_expansions), (1, 1));
    let complete = settled(&mut s, sheet);
    assert!(complete.stale_fills.is_empty() && complete.stale_sketches.is_empty());
    assert_eq!(
        complete.fills.iter().map(|fill| (fill.fill, fill.sketch, fill.status.name())).collect::<Vec<_>>(),
        expected.map(|id| (id, sketch, "drawn"))
    );
    assert_eq!((s.drawing_scenes.fill_solves, s.drawing_scenes.fill_expansions, s.drawing_scenes.sketch_solves), (1, 2, 1));
    assert_eq!(bytes(&complete.scene), bytes(&build(&s, s.doc.drawing(drawing).unwrap(), sheet).unwrap().scene));
    assert!(Arc::ptr_eq(&doc, &s.doc) && Arc::ptr_eq(&model, &s.model.state()));
    assert_eq!((s.evaluations, s.undo.len(), s.redo.len(), s.revision), counters);
    let again = extract(&mut s, sheet).unwrap();
    assert!(Arc::ptr_eq(&complete.scene, &again.scene));
    assert!(!step(&mut s));
}

#[test]
fn sheet_fill_provider_delete_undo_sheet_switch_and_reopen_forget_the_right_entries() {
    let (mut s, drawing, sheet, sketch, boundary) = w38d_scene_fixture();
    let fill = w38d_create_fill(&mut s, sketch, &boundary, json!({"kind":"solid"}));
    let other = run(&mut s, "drawing.sheet.add", json!({"name":"Other"}))["sheet"].as_str().and_then(ids::decode).unwrap();
    let before = settled(&mut s, sheet);
    let count = s.drawing_scenes.fill_solves;
    let blank = settled(&mut s, other);
    assert!(blank.fills.is_empty() && blank.stale_fills.is_empty());
    assert_eq!(bytes(&settled(&mut s, sheet).scene), bytes(&before.scene));
    assert_eq!(s.drawing_scenes.fill_solves, count, "switching sheets does not change a source dependency");
    run(&mut s, "drawing.fill.delete", json!({"fill":ids::encode(fill)}));
    let deleted = extract(&mut s, sheet).unwrap();
    assert!(deleted.fills.is_empty() && deleted.scene.groups.iter().all(|group| group.kind != ObjectKind::Fill));
    assert!(!s.drawing_scenes.fills.contains_key(&fill) && !s.drawing_scenes.fill_regions.contains_key(&sketch));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(extract(&mut s, sheet).unwrap().stale_fills, [fill]);
    assert_eq!(bytes(&settled(&mut s, sheet).scene), bytes(&before.scene));
    run(&mut s, "drawing.close", json!({}));
    assert!(s.drawing_scenes.fills.is_empty() && s.drawing_scenes.fill_regions.is_empty());
    run(&mut s, "drawing.open", json!({"drawing":ids::encode(drawing)}));
    assert_eq!(extract(&mut s, sheet).unwrap().stale_fills, [fill]);
    assert_eq!(bytes(&settled(&mut s, sheet).scene), bytes(&before.scene));
    run(&mut s, "drawing.sheet.delete", json!({"sheet":ids::encode(sheet),"contents":"delete"}));
    assert!(!step(&mut s));
    assert!(extract(&mut s, sheet).is_err());
    assert!(settled(&mut s, other).fills.is_empty());
    assert!(s.drawing_scenes.fills.is_empty() && s.drawing_scenes.fill_regions.is_empty());
}

#[test]
fn sheet_fill_paints_between_views_and_sketches_in_physical_paper_coordinates() {
    let (mut s, sheet, [front, _]) = front_and_top();
    let drawing = s.open_drawing.unwrap();
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "document.units", json!({"units":"in"}));
    run(&mut s, "drawing.open", json!({"drawing":ids::encode(drawing)}));
    run(&mut s, "drawing.set", json!({"units":"in"}));
    let created = run(&mut s, "drawing.sketch.create", json!({"owner":ids::encode(sheet)}));
    let sketch = ids::decode(created["sketch"].as_str().unwrap()).unwrap();
    run(&mut s, "sketch.rectangle.two_point", json!({"p0":[15,10],"p1":[55,30]}));
    run(&mut s, "sketch.finish", json!({}));
    let boundary = run(&mut s, "drawing.fill.profiles", json!({"sketch":ids::encode(sketch)}))["fill_profiles"]["boundaries"][0].clone();
    let fill = w38d_create_fill(&mut s, sketch, &boundary, json!({"kind":"solid"}));
    let before = settled(&mut s, sheet);
    let kinds: Vec<_> = before.scene.groups.iter().map(|group| group.kind).collect();
    assert_eq!(&kinds[kinds.len() - 4..], &[ObjectKind::View, ObjectKind::View, ObjectKind::Fill, ObjectKind::Sketch]);
    let paint = before.scene.groups.iter().find(|group| group.object == fill && group.kind == ObjectKind::Fill).unwrap();
    let Item::Fill { region } = &paint.items[0] else { panic!("solid region disappeared") };
    assert!((region.area() - 800.0).abs() < 1e-7);
    assert!(region.outer.segs.iter().any(|seg| seg.start().dist(Vec2::new(15.0, 10.0)) < 1e-9));
    let projections = s.drawing_scenes.projections;
    let solves = s.drawing_scenes.fill_solves;
    let data = serde_json::to_string(paint).unwrap();
    run(&mut s, "drawing.view.edit", json!({"view":ids::encode(front),"set":{"place":{"at":[200,150]}}}));
    let moved = extract(&mut s, sheet).unwrap();
    assert!(moved.stale_fills.is_empty());
    assert_eq!((s.drawing_scenes.projections, s.drawing_scenes.fill_solves), (projections, solves));
    assert_eq!(
        serde_json::to_string(moved.scene.groups.iter().find(|group| group.object == fill && group.kind == ObjectKind::Fill).unwrap()).unwrap(),
        data
    );
    assert!(!step(&mut s));
}

#[test]
fn sheet_fill_provider_failed_source_is_cached_and_draws_no_retained_fill() {
    let (mut s, drawing, sheet, sketch, _) = w38d_scene_fixture();
    // Setters refuse unknown expressions. Start from a real, valid global
    // source, then remove it only after the initial fill has been certified.
    s.doc_mut().set_param("MissingFillSource", "Inset", Some("mm"), None).unwrap();
    let mut owner = (**s.doc.drawing(drawing).unwrap()).clone();
    for point in Arc::make_mut(&mut owner.sketches[0]).sketch.points.iter_mut().skip(1) {
        point.exprs = Some(Box::new(solidcraft_sketch::PointExprs { x: Some(format!("MissingFillSource + {} mm", point.pos.x - 25.4)), y: None }));
    }
    s.doc_mut().put_drawing(Arc::new(owner));
    let boundary = run(&mut s, "drawing.fill.profiles", json!({"sketch":ids::encode(sketch)}))["fill_profiles"]["boundaries"][0].clone();
    let fill = w38d_create_fill(&mut s, sketch, &boundary, json!({"kind":"solid"}));
    let before = settled(&mut s, sheet);
    assert_eq!(before.fills.len(), 1);
    assert_eq!(before.fills[0].status, FillStatus::Drawn);
    assert!(before.scene.groups.iter().any(|group| group.kind == ObjectKind::Fill && group.object == fill));
    let retained = s.doc.drawing(drawing).unwrap().fills[0].last_region.clone();
    assert!(s.doc.param_users("MissingFillSource").is_empty(), "sheet expressions are resolved from declared source definitions");
    s.doc_mut().remove_param("MissingFillSource").unwrap();
    assert!(s.doc.param("MissingFillSource").is_none());
    let doc = s.doc.clone();
    let evaluations = s.evaluations;
    let failed = settled(&mut s, sheet);
    assert!(matches!(&failed.fills[0].status, FillStatus::Failed(why) if why.contains("MissingFillSource") && why.contains(&ids::encode(fill))));
    assert!(failed.scene.groups.iter().all(|group| group.kind != ObjectKind::Fill));
    assert_eq!(s.doc.drawing(drawing).unwrap().fills[0].last_region, retained);
    let solves = s.drawing_scenes.fill_solves;
    assert!(!step(&mut s));
    assert_eq!(s.drawing_scenes.fill_solves, solves);
    assert!(Arc::ptr_eq(&doc, &s.doc) && s.evaluations == evaluations);
    s.doc_mut().set_param("MissingFillSource", "2 in", Some("in"), None).unwrap();
    let repaired = settled(&mut s, sheet);
    assert_eq!(repaired.fills[0].status, FillStatus::Drawn);
    assert_eq!(s.drawing_scenes.fill_solves, solves + 1);
    assert_eq!(s.evaluations, evaluations);
}

#[test]
fn sheet_fill_named_feature_source_change_updates_retention_without_drawing_history() {
    let (mut s, drawing, sheet, sketch, _) = w38d_scene_fixture();
    let feature = s.doc.features.iter().find(|feature| !feature.param_names.is_empty()).unwrap();
    let name = feature.param_names[0].clone();
    let mut owner = (**s.doc.drawing(drawing).unwrap()).clone();
    for point in Arc::make_mut(&mut owner.sketches[0]).sketch.points.iter_mut().skip(1) {
        point.exprs = Some(Box::new(solidcraft_sketch::PointExprs { x: Some(format!("{name} + {} mm", point.pos.x - 40.0)), y: None }));
    }
    s.doc_mut().put_drawing(Arc::new(owner));
    let boundary = run(&mut s, "drawing.fill.profiles", json!({"sketch":ids::encode(sketch)}))["fill_profiles"]["boundaries"][0].clone();
    let fill = w38d_create_fill(&mut s, sketch, &boundary, json!({"kind":"solid"}));
    let before = s.doc.drawing(drawing).unwrap().fills[0].last_region.clone();
    assert!(before.outer.segs.iter().any(|seg| seg.start().dist(Vec2::new(15.0, 10.0)) < 1e-7));
    let user_params = s.doc.params.clone();
    let drawing_steps = s.undo.iter().filter(|step| step.part == Some(drawing)).count();
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "parameters.change", json!({"name":name,"expression":"60 mm"}));
    assert_eq!(s.doc.params, user_params, "named feature inputs are outside user parameter storage");
    assert_eq!(s.undo.iter().filter(|step| step.part == Some(drawing)).count(), drawing_steps);
    let newest = s.doc.drawing(drawing).unwrap().sheet_fill(fill).unwrap().last_region.clone();
    assert!(newest.outer.segs.iter().any(|seg| seg.start().dist(Vec2::new(35.0, 10.0)) < 1e-7));
    run(&mut s, "drawing.open", json!({"drawing":ids::encode(drawing)}));
    run(&mut s, "drawing.sketch.edit", json!({"sketch":ids::encode(sketch)}));
    run(&mut s, "sketch.delete", json!({"entities":[boundary["outer"][0]]}));
    run(&mut s, "sketch.finish", json!({}));
    let lost = settled(&mut s, sheet);
    assert!(matches!(&lost.fills[0].status, FillStatus::LostBoundary(_)));
    assert_eq!(s.doc.drawing(drawing).unwrap().sheet_fill(fill).unwrap().last_region, newest);
    let mut reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    run(&mut reopened, "drawing.open", json!({"drawing":ids::encode(drawing)}));
    let restored = settled(&mut reopened, sheet);
    assert!(matches!(&restored.fills[0].status, FillStatus::LostBoundary(_)));
    assert_eq!(bytes(&restored.scene), bytes(&lost.scene));
}

#[test]
fn sheet_fill_dots_are_physical_disks_and_unrepresentable_widths_refuse() {
    use solidcraft_doc::drawing::FillPaint;
    let (mut s, drawing, _, sketch, boundary) = w38d_scene_fixture();
    let imported = run(&mut s, "drawing.pattern.import", json!({"kind":"pat","source":"*DOTS,Original dots\n0,0,0,0,5,0,-5\n\n"}));
    let pattern = ids::decode(imported["patterns"][0]["id"].as_str().unwrap()).unwrap();
    let fill_id = w38d_create_fill(&mut s, sketch, &boundary, json!({"kind":"hatch","pattern":ids::encode(pattern),"pen_width_mm":0.25}));
    let owner = s.doc.drawing(drawing).unwrap();
    let resolved = owner.resolve_sheet_fill(&s.doc, fill_id).unwrap();
    let group = fill::paper_group(owner, &resolved).unwrap();
    assert_eq!(group.kind.layer(), "FILL");
    assert_eq!(group.items.len(), 21, "seven strict-interior dots on each of y=15,20,25");
    let mut centers = Vec::new();
    for item in &group.items {
        let Item::Fill { region } = item else { panic!("a dot became a stroke") };
        assert!(region.holes.is_empty());
        assert!((region.area() - std::f64::consts::PI * 0.125f64.powi(2)).abs() < 1e-10);
        let Seg2::Arc { center, radius, .. } = region.outer.segs[0] else { panic!("a disk was sampled") };
        assert_eq!(radius, 0.125);
        centers.push((center.x as i32, center.y as i32));
    }
    centers.sort_unstable();
    let expected: Vec<_> = (4..=10).flat_map(|x| (3..=5).map(move |y| (x * 5, y * 5))).collect();
    assert_eq!(centers, expected);
    let scene = Scene::new(Vec2::new(80.0, 50.0), vec![group]);
    let prepared = scene.prepare().unwrap();
    let svg = solidcraft_io::paper::paper_svg(&prepared, &NoPictures, 300.0).unwrap().0;
    assert_eq!(String::from_utf8(svg).unwrap().matches("fill-rule=\"evenodd\"").count(), 21);
    let mut png = Vec::new();
    let report = solidcraft_io::paper::paper_png(
        &prepared,
        solidcraft_io::paper::PngOptions { dpi: 254.0, colour: solidcraft_io::paper::PngColour::Grey },
        &NoPictures,
        &mut png,
    )
    .unwrap();
    assert_eq!(report.pixels, [800, 500]);
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    for width in [f64::from_bits(1), 1e-11, 1e-6, 1e-4] {
        let mut candidate = (**owner).clone();
        Arc::make_mut(candidate.fills.iter_mut().find(|fill| fill.id == fill_id).unwrap()).paint = FillPaint::Hatch { pattern, pen_width_mm: width };
        let selected = candidate.resolve_sheet_fill(&s.doc, fill_id).unwrap();
        let why = fill::paper_group(&candidate, &selected).unwrap_err().to_string();
        assert!(why.contains("dot") && why.contains(&ids::encode(fill_id)), "{width}: {why}");
    }
    let mut candidate = (**owner).clone();
    Arc::make_mut(candidate.fills.iter_mut().find(|fill| fill.id == fill_id).unwrap()).paint = FillPaint::Hatch { pattern, pen_width_mm: 1e-11 };
    let mut selected = candidate.resolve_sheet_fill(&s.doc, fill_id).unwrap();
    for seg in &mut selected.region.outer.segs {
        *seg = seg.mapped(|p| p + Vec2::new(99900.0, 99900.0), solidcraft_geom::ArcMap::turn(0.0, false));
    }
    let why = fill::paper_group(&candidate, &selected).unwrap_err().to_string();
    assert!(why.contains("filled disk at the dot centre"), "{why}");
}

/// A hatch dot is held to every grid a paper writer draws on, as io lists them, finest first: a dot
/// of 5 steps' radius on a grid (its quarter turns' chords merge there) is refused naming that grid.
#[test]
fn sheet_fill_dots_are_checked_on_every_paper_writers_grid() {
    use solidcraft_doc::drawing::FillPaint;
    let (mut s, drawing, _, sketch, boundary) = w38d_scene_fixture();
    let imported = run(&mut s, "drawing.pattern.import", json!({"kind":"pat","source":"*DOTS,Original dots\n0,0,0,0,5,0,-5\n\n"}));
    let pattern = ids::decode(imported["patterns"][0]["id"].as_str().unwrap()).unwrap();
    let fill_id = w38d_create_fill(&mut s, sketch, &boundary, json!({"kind":"hatch","pattern":ids::encode(pattern),"pen_width_mm":0.25}));
    let owner = s.doc.drawing(drawing).unwrap();
    let grids = solidcraft_io::paper::writer_grids().unwrap();
    assert!(grids.windows(2).all(|w| w[0].per_mm() > w[1].per_mm()), "finest first");
    for grid in grids {
        let mut candidate = (**owner).clone();
        let pen_width_mm = 10.0 / grid.per_mm();
        Arc::make_mut(candidate.fills.iter_mut().find(|fill| fill.id == fill_id).unwrap()).paint = FillPaint::Hatch { pattern, pen_width_mm };
        let selected = candidate.resolve_sheet_fill(&s.doc, fill_id).unwrap();
        let why = fill::paper_group(&candidate, &selected).unwrap_err().to_string();
        assert!(why.contains(&format!("the {} steps/mm grid", grid.per_mm())), "{pen_width_mm}: {why}");
    }
}

#[test]
fn sheet_fill_solid_regions_keep_conic_and_quartic_bezier_geometry_exact() {
    use solidcraft_sketch::{CurveKind, Sketch};
    for conic in [true, false] {
        let (mut s, drawing, sheet, sketch, _) = w38d_scene_fixture();
        let mut owner = (**s.doc.drawing(drawing).unwrap()).clone();
        let mut geometry = Sketch::new();
        let controls = if conic {
            vec![[10.0, 10.0], [20.0, 30.0], [30.0, 10.0]]
        } else {
            vec![[10.0, 10.0], [10.0, 30.0], [20.0, 40.0], [30.0, 30.0], [30.0, 10.0]]
        };
        let points: Vec<_> = controls.iter().map(|p| geometry.add_point(Vec2::new(p[0], p[1]), None).unwrap()).collect();
        let first = points[0];
        let last = *points.last().unwrap();
        let kind = if conic {
            CurveKind::Conic { a: first, b: last, apex: points[1], rho: 0.4 }
        } else {
            CurveKind::Spline { pts: points, degree: 4, control: true, periodic: false, nurbs: None, handles: Vec::new() }
        };
        geometry.add_curve(kind, Some("cap")).unwrap();
        geometry.add_line_pts(last, first, Some("closing")).unwrap();
        for point in &mut geometry.points {
            point.fixed = true;
        }
        for curve in &mut geometry.curves {
            curve.fixed = true;
        }
        Arc::make_mut(&mut owner.sketches[0]).sketch = geometry;
        s.doc_mut().put_drawing(Arc::new(owner));
        let profiles = s.doc.drawing(drawing).unwrap().sheet_fill_profiles(&s.doc, sketch).unwrap();
        assert_eq!(profiles.uncertified, 0);
        assert_eq!(profiles.profiles.len(), 1);
        let boundary = serde_json::to_value(&profiles.profiles[0].boundary).unwrap();
        let id = w38d_create_fill(&mut s, sketch, &boundary, json!({"kind":"solid"}));
        let scene = settled(&mut s, sheet);
        assert_eq!(scene.fills[0].status, FillStatus::Drawn);
        let group = scene.scene.groups.iter().find(|group| group.object == id && group.kind == ObjectKind::Fill).unwrap();
        let Item::Fill { region } = &group.items[0] else { panic!("solid fill lost its exact region") };
        let curve = region
            .outer
            .segs
            .iter()
            .find(|seg| if conic { matches!(seg, Seg2::Conic { .. }) } else { matches!(seg, Seg2::Bezier { n: 4, .. }) })
            .unwrap();
        let expected = Vec2::new(20.0, if conic { 18.0 } else { 31.25 });
        assert!(curve.point_at(0.5).dist(expected) < 1e-8, "analytic midpoint: {:?}", curve.point_at(0.5));
        // Current A3b admits this exact conic/quartic fill. Inspect its own SVG
        // group, so a surviving border or sketch stroke cannot mask a dropped fill.
        let prepared = scene.scene.prepare().unwrap();
        let written = solidcraft_io::paper::paper_svg(&prepared, &NoPictures, 300.0).unwrap().0;
        assert_eq!(solidcraft_io::paper::paper_svg(&prepared, &NoPictures, 300.0).unwrap().0, written, "deterministic actual fill export");
        let svg = std::str::from_utf8(&written).unwrap();
        let tag = format!("<g data-object=\"{id}\" data-kind=\"fill\">");
        assert_eq!(svg.matches(tag.as_str()).count(), 1, "the authored fill has its own layer-kind group");
        let painted = svg.split_once(tag.as_str()).unwrap().1.split_once("</g>").unwrap().0;
        assert_eq!(painted.matches("<path ").count(), 1, "one actual filled contour");
        assert!(painted.contains("<path fill=\"#000\" stroke=\"none\" fill-rule=\"evenodd\" d=\""), "opaque black even-odd fill: {painted}");
        assert!(!svg.contains("opacity=") && !painted.contains("<image"), "the fixture is painted at default full opacity");
        let data = painted.split_once(" d=\"").unwrap().1.split_once('"').unwrap().0;
        assert_eq!(data.matches('M').count(), 1);
        assert_eq!(data.matches('Z').count(), 1);
        assert!(data.ends_with('Z'), "the fill remains closed");
        let commands: Vec<_> = data.char_indices().filter(|(_, op)| op.is_ascii_alphabetic()).collect();
        let baseline = scene.scene.size_mm.y - 10.0;
        let (left, right) = (Vec2::new(10.0, baseline), Vec2::new(30.0, baseline));
        let (mut current, mut first_curve, mut last_curve) = (None, None, None);
        let (mut curves, mut lines, mut visible_curve) = (0usize, 0usize, false);
        for (at, &(start, op)) in commands.iter().enumerate() {
            let end = commands.get(at + 1).map_or(data.len(), |(next, _)| *next);
            let points: Vec<f64> = data[start + 1..end].split_whitespace().map(|number| number.parse().unwrap()).collect();
            assert!(points.iter().all(|number| number.is_finite()));
            match op {
                'M' => {
                    assert_eq!(points.len(), 2);
                    current = Some(Vec2::new(points[0], points[1]));
                }
                'C' => {
                    assert_eq!(points.len(), 6);
                    if first_curve.is_none() {
                        first_curve = Some(current.unwrap());
                    }
                    let end = Vec2::new(points[4], points[5]);
                    last_curve = Some(end);
                    current = Some(end);
                    visible_curve |= points[1] < baseline - 1.0 || points[3] < baseline - 1.0;
                    curves += 1;
                }
                'L' => {
                    assert_eq!(points.len(), 2);
                    let end = Vec2::new(points[0], points[1]);
                    let start = current.unwrap();
                    assert!((start == left && end == right) || (start == right && end == left));
                    current = Some(end);
                    lines += 1;
                }
                'Z' => assert!(points.is_empty()),
                _ => panic!("unexpected command in the actual conic/quartic filled path: {op}"),
            }
        }
        let (a, b) = (first_curve.unwrap(), last_curve.unwrap());
        assert!((a == left && b == right) || (a == right && b == left), "the actual curve retains both authored endpoints");
        assert!(curves > 0 && lines <= 1 && visible_curve, "the curved cap is visible; Z may supply its one closing line");
        let grid = prepared.on_grid(solidcraft_geom::paper::Grid::NANOMETRE).unwrap();
        let target = grid.groups.iter().find(|group| group.object == id && group.kind == ObjectKind::Fill).unwrap();
        let solidcraft_geom::paper::GridItem::Fill { loops } = &target.items[0] else { panic!("filled grid contour") };
        assert_eq!(loops.len(), 1);
        let cubics = loops[0].pieces.iter().filter(|piece| matches!(piece, solidcraft_geom::paper::Piece::Cubic { .. })).count();
        assert_eq!(curves, cubics, "every admitted curve piece is emitted in the fill's own path");
        let reopened = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
        assert_eq!(&reopened.drawing(drawing).unwrap().sheet_fill(id).unwrap().last_region, region);
    }
}

#[test]
fn held_sheet_fills_survive_close_reopen_and_scratch_export() {
    let (mut s, drawing, sheet, sketch, boundary) = w38d_scene_fixture();
    let fill = w38d_create_fill(&mut s, sketch, &boundary, json!({"kind":"solid"}));
    run(&mut s, "drawing.set", json!({"updates":"deferred"}));
    run(&mut s, "drawing.update", json!({}));
    let before = settled(&mut s, sheet);
    let paint = bytes_of(group_of(&before.scene, fill));
    run(&mut s, "drawing.close", json!({}));
    assert!(s.drawing_scenes.fills.is_empty() && s.drawing_scenes.fill_regions.is_empty());
    let kept = s.drawing_scenes.kept.of(drawing).unwrap();
    assert_eq!(bytes_of(kept.fills[&fill].drawn.group.as_ref().unwrap()), paint);
    run(&mut s, "drawing.open", json!({"drawing":ids::encode(drawing)}));
    let export = json!({"sheet":ids::encode(sheet),"format":"svg"});
    let there = s.scratch_as_shown().execute("drawing.export", &export).unwrap();
    let here = run(&mut s, "drawing.export", export);
    assert_eq!((&there["svg"], &there["out_of_date"]), (&here["svg"], &here["out_of_date"]));
    assert!(here["out_of_date"].as_array().unwrap().contains(&json!(ids::encode(fill))), "{here}");
    let held = extract(&mut s, sheet).unwrap();
    assert_eq!(bytes_of(group_of(&held.scene, fill)), paint);
    assert_eq!(held.stale_fills, [fill]);
    assert!(!step(&mut s));
    run(&mut s, "drawing.update", json!({}));
    let current = settled(&mut s, sheet);
    assert!(current.stale_fills.is_empty());
    assert_eq!(bytes_of(group_of(&current.scene, fill)), paint);
}

#[test]
fn sheet_spline_retains_nonuniform_weighted_spans() {
    use solidcraft_sketch::{CurveKind, Nurbs, Sketch};
    let (s, owner, id) = stroke_editor();
    let mut solved = s.doc.solve_sheet_sketch(owner, id).unwrap();
    let mut geometry = Sketch::new();
    let pts = [(10.0, 10.0), (20.0, 30.0), (30.0, 20.0), (40.0, 30.0), (50.0, 10.0)]
        .into_iter()
        .map(|(x, y)| geometry.add_point(Vec2::new(x, y), None).unwrap())
        .collect();
    geometry
        .add_curve(
            CurveKind::Spline {
                pts,
                control: true,
                degree: 2,
                periodic: false,
                handles: Vec::new(),
                nurbs: Some(Nurbs { knots: vec![0.0, 0.0, 0.0, 0.5, 0.5, 1.0, 1.0, 1.0], weights: vec![1.0, 1.0, 0.5, 1.0, 1.0] }),
            },
            None,
        )
        .unwrap();
    solved.sketch = geometry;
    let group = sketch::paper_group(&solved, 0.5).unwrap();
    let Item::Stroke { path, .. } = &group.items[0] else { panic!("expected spline stroke") };
    assert_eq!(path.len(), 2, "repeated interior knots create two exact spans, not three");
    assert!(path[0].start().dist(Vec2::new(10.0, 10.0)) < 1e-9);
    assert!(path[0].end().dist(path[1].start()) < 1e-9);
    assert!(path[1].end().dist(Vec2::new(50.0, 10.0)) < 1e-9);
}

#[test]
fn sheet_spline_handles_are_retained_and_bounded() {
    use solidcraft_sketch::{CurveKind, Sketch};
    let (s, owner, id) = stroke_editor();
    let mut solved = s.doc.solve_sheet_sketch(owner, id).unwrap();
    let mut geometry = Sketch::new();
    let pts = [(10.0, 10.0), (20.0, 20.0), (30.0, 10.0)].into_iter().map(|(x, y)| geometry.add_point(Vec2::new(x, y), None).unwrap()).collect();
    let handles = [(12.0, 10.0), (22.0, 20.0), (32.0, 10.0)].into_iter().map(|(x, y)| geometry.add_point(Vec2::new(x, y), None).unwrap()).collect();
    geometry.add_curve(CurveKind::Spline { pts, control: false, degree: 3, periodic: false, nurbs: None, handles }, None).unwrap();
    solved.sketch = geometry;
    let group = sketch::paper_group(&solved, 0.5).unwrap();
    let Item::Stroke { path, .. } = &group.items[0] else { panic!("expected spline stroke") };
    assert_eq!(path.len(), 2);
    let Seg2::Cubic { p0, p1, .. } = path[0] else { panic!("expected cubic") };
    assert!(p1.x > p0.x && (p1.y - p0.y).abs() < 1e-9, "authored horizontal tangent retained");
    if let CurveKind::Spline { handles, .. } = &mut solved.sketch.curves[0].kind {
        *handles = vec![usize::MAX; 501];
    }
    assert!(sketch::paper_group(&solved, 0.5).unwrap_err().to_string().contains("tangent handles"));
}

#[test]
fn sheet_centerline_refusal_preserves_exportable_strokes_and_history() {
    let (mut s, owner, id) = stroke_editor();
    let line = run(&mut s, "sketch.line", json!({"points":[[10,10],[30,10]],"infer":false}))["curves"][0].clone();
    let original = sketch::paper_group(&s.doc.solve_sheet_sketch(owner, id).unwrap(), 0.5).unwrap();
    let before = s.doc.clone();
    let history = (s.undo.len(), s.redo.len(), s.evaluations);
    for params in [json!({"curves":[line],"value":true}), json!({"curves":[line]})] {
        let why = s.execute("sketch.centerline", &params).unwrap_err().to_string();
        assert!(why.contains("centerline"), "{why}");
        assert!(Arc::ptr_eq(&before, &s.doc));
        assert_eq!((s.undo.len(), s.redo.len(), s.evaluations), history);
        let reopened = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
        let group = sketch::paper_group(&reopened.solve_sheet_sketch(owner, id).unwrap(), 0.5).unwrap();
        assert_eq!(group, original);
        let scene = Scene::new(Vec2::new(420.0, 297.0), vec![group]);
        let svg = solidcraft_io::paper::paper_svg(&scene.prepare().unwrap(), &NoPictures, DEFAULT_DPI).unwrap().0;
        assert!(String::from_utf8(svg).unwrap().contains("data-kind=\"sketch\""));
    }
}

/// A rule's K-factor changes the flat pattern: after the change, the plate's flat view is out of date
/// and draws again, beside its base view. A change the flat doesn't show (a sketch: a new world state
/// that adds no body) keeps it, as it keeps every view.
#[test]
fn a_rule_change_draws_the_flat_view_again() {
    let mut s = plate();
    let sheet = s.drawing("test").unwrap().sheets[0].id;
    let made = |s: &mut Session, command: &str, p: Value| ids::decode(run(s, command, p)["view"].as_str().unwrap()).unwrap();
    let flat = made(&mut s, "drawing.view.flat", json!({"sheet": "Sheet 1", "scale": "1:2", "position_mm": [300, 150]}));
    made(&mut s, "drawing.view.base", json!({"sheet": "Sheet 1", "scale": "1:2", "position_mm": [100, 150]}));
    settled(&mut s, sheet);
    let drawing = json!({"drawing": ids::encode(s.open_drawing.unwrap())});
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Note"}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "drawing.open", drawing.clone());
    assert!(extract(&mut s, sheet).unwrap().stale.is_empty());
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "sheet.manage_rules", json!({"name": "T2", "k_factor": 0.3}));
    run(&mut s, "drawing.open", drawing);
    let stale = extract(&mut s, sheet).unwrap().stale;
    assert!(stale.contains(&flat), "{stale:?}");
    settled(&mut s, sheet);
    assert!(extract(&mut s, sheet).unwrap().stale.is_empty());
}

/// [`front_and_top`]'s block with a hole drilled Ø6.8 16 deep down from the middle of its top, and an
/// M8 cosmetic thread 12 deep in it (the feature `Tap`): a thread of its own, so editing it changes
/// no geometry.
fn tapped_front_and_top() -> (Session, u64, [u64; 2]) {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 100, "width": 50, "height": 20, "body_name": "Block"}));
    run(&mut s, "solid.hole", json!({"position": [50, 25, 20], "direction": [0, 0, -1], "diameter": 6.8, "depth": 16}));
    run(&mut s, "solid.thread", json!({"face": [53.4, 25, 15], "designation": "M8", "length": 12, "from": [53.4, 25, 20], "name": "Tap"}));
    front_and_top_of(s)
}

/// The arcs of `group`'s strokes of `radius` (paper millimetres).
fn arcs_of(group: &Group, radius: f64) -> usize {
    let segs = group.items.iter().filter_map(|item| match item {
        Item::Stroke { path, .. } => Some(path),
        _ => None,
    });
    segs.flatten().filter(|seg| matches!(seg, Seg2::Arc { radius: r, .. } if (r - radius).abs() < 1e-9)).count()
}

/// A sheet's view draws a cosmetic thread's lines as its snapshot does: from the front, hidden
/// (two root lines and the limit line, dashed); from above, the root's three-quarter arc, radius 2
/// on paper at 1:2. Changing only the thread's depth draws the front view again, with its limit line
/// moved, though the block's geometry is the same.
#[test]
fn a_views_threads_draw_as_its_snapshot_does_and_follow_the_thread() {
    let (mut s, sheet, [front, top]) = tapped_front_and_top();
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(front), "set": {"visual_style": "visible_hidden"}}));
    let scene = settled(&mut s, sheet).scene;
    let group = |scene: &Scene, id: u64| scene.groups.iter().find(|g| g.object == id).unwrap().clone();
    let ours = group(&scene, front);
    assert_eq!(arcs_of(&group(&scene, top), 2.0), 1, "{:?}", group(&scene, top));
    let dashed = ours.items.iter().filter(|item| matches!(item, Item::Stroke { pattern: Some(_), .. })).count();
    // The drill's two sides and its floor, its cone's two sides, and the thread's three lines.
    assert!(dashed >= 6, "{ours:?}");
    let drawing = ids::encode(s.open_drawing.unwrap());
    run(&mut s, "drawing.close", json!({}));
    let p = json!({"view": "front", "scale": "1:2", "position_mm": [150, 150], "scene": true, "visual_style": "visible_hidden"});
    let v = run(&mut s, "drawing.view.snapshot", p);
    let snapped: Scene = serde_json::from_value(v["scene"].clone()).unwrap();
    assert!(same_within(&ours, &snapped.groups[0], 1e-9), "{ours:?} {:?}", snapped.groups[0]);
    assert_eq!(v["threads"].as_array().map(Vec::len), Some(1), "{v}");
    let geometry = s.world_state().unwrap().bodies[0].geometry_key();
    run(&mut s, "timeline.edit", json!({"feature": "Tap", "set": {"length": "8"}}));
    assert_eq!(s.world_state().unwrap().bodies[0].geometry_key(), geometry, "a cosmetic thread changes no geometry");
    run(&mut s, "drawing.open", json!({"drawing": drawing}));
    let moved = group(&settled(&mut s, sheet).scene, front);
    assert_ne!(bytes_of(&moved), bytes_of(&ours), "the limit line moves with the thread");
    run(&mut s, "drawing.close", json!({}));
    let v = run(
        &mut s,
        "drawing.view.snapshot",
        json!({"view": "front", "scale": "1:2", "position_mm": [150, 150], "scene": true, "visual_style": "visible_hidden"}),
    );
    let snapped: Scene = serde_json::from_value(v["scene"].clone()).unwrap();
    assert!(same_within(&moved, &snapped.groups[0], 1e-9), "{moved:?} {:?}", snapped.groups[0]);
}

/// Two placements of a tapped plate each draw their own thread: two root arcs from above.
#[test]
fn each_placement_of_a_tapped_part_draws_its_thread() {
    let mut s = Session::default();
    let plate = run(&mut s, "component.create", json!({"name": "Plate"}));
    run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 10, "body_name": "PlateBody"}));
    run(&mut s, "solid.hole", json!({"position": [20, 20, 10], "direction": [0, 0, -1], "thread": "M8", "depth": 8}));
    run(&mut s, "component.activate", json!({"component": 0}));
    run(&mut s, "occurrence.copy", json!({"component": plate["component"], "translate": [60, 0, 0]}));
    let (mut s, sheet, [_, top]) = front_and_top_of(s);
    let scene = settled(&mut s, sheet).scene;
    let top = scene.groups.iter().find(|g| g.object == top).unwrap();
    assert_eq!(arcs_of(top, 2.0), 2, "{top:?}");
}

/// A shaded block, its front view at 1:1 centred at `at` on the sheet: the session, the sheet and
/// the view.
fn shaded_front(at: [f64; 2]) -> (Session, u64, u64) {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 100, "width": 50, "height": 20, "body_name": "Block"}));
    run(&mut s, "drawing.create", json!({}));
    let p = json!({"sheet": "Sheet 1", "view": "front", "scale": "1:1", "position_mm": at, "visual_style": "shaded"});
    let id = ids::decode(run(&mut s, "drawing.view.base", p)["view"].as_str().unwrap()).unwrap();
    let sheet = s.drawing("test").unwrap().sheets[0].id;
    (s, sheet, id)
}

/// The one picture of a sheet's answer.
fn only_picture(answer: &SheetView) -> &SheetPicture {
    let [picture] = &answer.pictures[..] else { panic!("{:?}", answer.pictures) };
    picture
}

/// A step's picture budget of `k` bands: `spent` says so when asked after the `k`th.
fn bands(k: usize) -> impl FnMut() -> bool {
    let mut asked = 0;
    move || {
        asked += 1;
        asked >= k
    }
}

/// A shaded view's picture is drawn after its lines, one band a step on a budget of one band, and
/// handed on only once whole: until then it has no rows and the answer says a picture is being
/// drawn. At the sheet's corner, so its frame is the export's, its rows are the export's on the same
/// grid, byte for byte.
#[test]
fn a_shaded_views_picture_is_drawn_a_band_a_step_and_handed_on_whole() {
    let (mut s, sheet, id) = shaded_front([0.0, 0.0]);
    let mut answer = extract(&mut s, sheet).unwrap();
    assert!(answer.pictures.is_empty() && !answer.drawing_pictures, "no picture before its view is drawn");
    while !answer.stale.is_empty() {
        assert!(step(&mut s), "its picture waits after its lines");
        answer = extract(&mut s, sheet).unwrap();
    }
    let picture = only_picture(&answer);
    assert_eq!((picture.source, picture.view, picture.rows.is_none(), picture.stale), (id, id, true, true));
    assert!(answer.drawing_pictures);
    let mut bands = 0;
    while step_within(&mut s, &mut self::bands(1)) {
        bands += 1;
        assert!(bands < 100_000, "the picture never finished");
        let now = extract(&mut s, sheet).unwrap();
        assert!(only_picture(&now).rows.is_none() && now.drawing_pictures, "never half a picture");
    }
    let done = extract(&mut s, sheet).unwrap();
    let picture = only_picture(&done);
    assert!(bands > 1 && !done.drawing_pictures && !picture.stale && picture.why.is_none(), "{bands} {picture:?}");
    let grid = picture.grid;
    assert_eq!(grid.pitch, 25.4 / DEFAULT_DPI);
    let rows = picture.rows.clone().unwrap();
    let d = s.drawing("test").unwrap().clone();
    let whole = GridWindow { x0: 0, y0: 0, x1: grid.width, y1: grid.height };
    let pictures = crate::drawing_picture::ViewPictures::new(&s, &d, sheet).unwrap();
    let export = solidcraft_geom::paper::Pictures::rows(&pictures, id, &grid, whole).unwrap();
    assert!(*rows == export[..], "the window's pixels are the export's");
    assert!(!step(&mut s));
}

/// A shaded section view draws no picture while nothing cuts its bodies (#13046): the sheet page's
/// pictures and an export's leave it out, and its parent's picture is drawn.
#[test]
fn an_uncut_section_view_draws_no_picture() {
    use solidcraft_doc::drawing::cutting_line::{ArrowSide, CuttingLine, SectionKind};
    let (mut s, sheet, front) = shaded_front([200.0, 150.0]);
    let mut d = (**s.drawing("test").unwrap()).clone();
    let cut = d.allocate_id(&mut solidcraft_doc::ids::IdSource::seeded(13046)).unwrap();
    let ends = vec![solidcore_record::Vec2::new(-60.0, 0.0), solidcore_record::Vec2::new(60.0, 0.0)];
    let line = CuttingLine::new(SectionKind::Full, ends, ArrowSide::Right).unwrap();
    let (kind, place) = (ViewKind::Section { parent: front, line }, ViewPlace::At([200.0, 60.0]));
    let view = View { id: cut, sheet, kind, place, settings: ViewOverrides::default(), reference_shown_percent: 0.0, label: None };
    d.add_views([Arc::new(view)]);
    d.validate().unwrap();
    s.doc_mut().put_drawing(Arc::new(d));
    extract(&mut s, sheet).unwrap();
    let mut steps = 0;
    while step(&mut s) {
        steps += 1;
        assert!(steps < 100_000, "the sheet never settled");
    }
    let answer = extract(&mut s, sheet).unwrap();
    assert_eq!(answer.pictures.iter().map(|picture| picture.view).collect::<Vec<_>>(), [front]);
    let d = s.drawing("test").unwrap().clone();
    let pictures = crate::drawing_picture::ViewPictures::new(&s, &d, sheet).unwrap();
    assert!(pictures.has(front) && !pictures.has(cut));
}

/// A flat pattern view draws only lines, even set to shaded: the sheet page's pictures and an
/// export's leave it out, and the shaded base view beside it has its picture.
#[test]
fn a_shaded_flat_pattern_view_draws_no_picture() {
    let mut s = plate();
    let sheet = s.drawing("test").unwrap().sheets[0].id;
    let made = |s: &mut Session, command: &str, p: Value| ids::decode(run(s, command, p)["view"].as_str().unwrap()).unwrap();
    let flat = made(&mut s, "drawing.view.flat", json!({"sheet": "Sheet 1", "scale": "1:2", "position_mm": [300, 150]}));
    let base = made(&mut s, "drawing.view.base", json!({"sheet": "Sheet 1", "scale": "1:2", "position_mm": [100, 150], "visual_style": "shaded"}));
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(flat), "set": {"visual_style": "shaded"}}));
    extract(&mut s, sheet).unwrap();
    let mut steps = 0;
    while step(&mut s) {
        steps += 1;
        assert!(steps < 100_000, "the sheet never settled");
    }
    let answer = extract(&mut s, sheet).unwrap();
    assert_eq!(answer.pictures.iter().map(|picture| picture.view).collect::<Vec<_>>(), [base]);
    let d = s.drawing("test").unwrap().clone();
    let pictures = crate::drawing_picture::ViewPictures::new(&s, &d, sheet).unwrap();
    assert!(pictures.has(base) && !pictures.has(flat));
}

/// A move draws no picture again: the same rows, current, on a grid moved by exactly the offset.
/// A design change that leaves the bodies as they look (a sheet added) keeps them too.
#[test]
fn a_moved_view_keeps_its_picture() {
    let (mut s, sheet, id) = shaded_front([200.0, 150.0]);
    let before = settled(&mut s, sheet);
    let (was, rows) = (only_picture(&before).grid, only_picture(&before).rows.clone().unwrap());
    let projections = s.drawing_scenes.projections;
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(id), "set": {"place": {"at": [210.0, 160.0]}}}));
    run(&mut s, "drawing.sheet.add", json!({}));
    let after = settled(&mut s, sheet);
    let picture = only_picture(&after);
    assert_eq!(s.drawing_scenes.projections, projections);
    assert!(Arc::ptr_eq(picture.rows.as_ref().unwrap(), &rows) && !picture.stale);
    assert!((picture.grid.x0 - was.x0 - 10.0).abs() < 1e-9 && (picture.grid.y0 - was.y0 - 10.0).abs() < 1e-9, "{:?}", picture.grid);
    assert_eq!((picture.grid.width, picture.grid.height, picture.grid.pitch), (was.width, was.height, was.pitch));
}

/// The last picture shows, stale, until the new one is whole: here after a model edit, made with the
/// drawing closed, which kept it.
#[test]
fn an_edited_model_shows_the_last_picture_stale_until_the_new_one_is_done() {
    let (mut s, sheet, _) = shaded_front([200.0, 150.0]);
    let drawing = json!({"drawing": ids::encode(s.drawing("test").unwrap().id)});
    let last = only_picture(&settled(&mut s, sheet)).rows.clone().unwrap();
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "solid.box", json!({"length": 40, "width": 60, "height": 15, "corner": [60, -5, 10], "operation": "cut", "targets": ["Block"]}));
    run(&mut s, "drawing.open", drawing);
    let mut answer = extract(&mut s, sheet).unwrap();
    while answer.drawing_pictures || !answer.stale.is_empty() {
        let picture = only_picture(&answer);
        assert!(picture.stale && Arc::ptr_eq(picture.rows.as_ref().unwrap(), &last), "the last picture shows until the new one");
        step(&mut s);
        answer = extract(&mut s, sheet).unwrap();
    }
    let picture = only_picture(&answer);
    assert!(!picture.stale && **picture.rows.as_ref().unwrap() != *last, "a new picture");
}

/// A picture is drawn again when its bodies' colours change, though its lines aren't: a checkpoint
/// gone back to with the drawing open gives back the block's first colour, and its first pixels.
#[test]
fn a_colour_taken_back_draws_the_picture_again_and_not_its_lines() {
    let (mut s, sheet, _) = shaded_front([200.0, 150.0]);
    let drawing = json!({"drawing": ids::encode(s.drawing("test").unwrap().id)});
    let first = only_picture(&settled(&mut s, sheet)).rows.clone().unwrap();
    let checkpoint = run(&mut s, "edit.checkpoint", json!({}))["checkpoint"].clone();
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "appearance.assign", json!({"bodies": ["Block"], "color": "#20a080"}));
    run(&mut s, "drawing.open", drawing);
    let green = only_picture(&settled(&mut s, sheet)).rows.clone().unwrap();
    assert!(*green != *first);
    let projections = s.drawing_scenes.projections;
    run(&mut s, "edit.restore_checkpoint", json!({"checkpoint": checkpoint}));
    let answer = extract(&mut s, sheet).unwrap();
    assert!(answer.stale.is_empty() && answer.drawing_pictures && only_picture(&answer).stale, "the lines are as they were, the picture isn't");
    let back = settled(&mut s, sheet);
    assert_eq!(s.drawing_scenes.projections, projections);
    assert!(**only_picture(&back).rows.as_ref().unwrap() == *first && !only_picture(&back).stale);
}

/// A picture is drawn again on its new grid when its sheet's dpi changes, though its lines aren't:
/// until then it shows stale, never as current pixels at the old dpi, which the export no longer uses.
#[test]
fn a_new_dpi_draws_the_picture_again_on_its_grid() {
    let (mut s, sheet, _) = shaded_front([200.0, 150.0]);
    assert_eq!(only_picture(&settled(&mut s, sheet)).grid.pitch, 25.4 / DEFAULT_DPI);
    let projections = s.drawing_scenes.projections;
    run(&mut s, "drawing.sheet.set", json!({"sheet": ids::encode(sheet), "dpi": DEFAULT_DPI / 2.0}));
    let answer = extract(&mut s, sheet).unwrap();
    assert!(answer.stale.is_empty() && answer.drawing_pictures && only_picture(&answer).stale, "the lines are as they were, the picture isn't");
    let after = settled(&mut s, sheet);
    let picture = only_picture(&after);
    assert_eq!(s.drawing_scenes.projections, projections, "no lines drawn again");
    assert!(!picture.stale && picture.why.is_none() && picture.grid.pitch == 25.4 / (DEFAULT_DPI / 2.0), "{:?}", picture.grid);
}

/// A picture asked for is dropped once its view changes: the next one is drawn for the view as it
/// is, and the half-drawn one is never handed on.
#[test]
fn a_view_edited_while_its_picture_is_drawn_draws_it_again() {
    let (mut s, sheet, id) = shaded_front([200.0, 150.0]);
    let mut answer = extract(&mut s, sheet).unwrap();
    while !answer.stale.is_empty() {
        step(&mut s);
        answer = extract(&mut s, sheet).unwrap();
    }
    step_within(&mut s, &mut bands(1));
    assert!(s.drawing_scenes.picturing.is_some(), "a picture is being drawn");
    let full = only_picture(&answer).grid;
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(id), "set": {"scale": "1:2"}}));
    let half = settled(&mut s, sheet);
    let picture = only_picture(&half);
    assert!(!picture.stale && picture.rows.is_some() && s.drawing_scenes.picturing.is_none());
    assert_eq!((picture.grid.width, picture.grid.height), (full.width.div_ceil(2), full.height.div_ceil(2)), "{full:?} {:?}", picture.grid);
}

/// A step draws a picture's bands until its budget is spent, never more than the band it was spent
/// in: on a budget of `k` bands, `k` a step, so the picture takes its bands divided by `k` steps,
/// rounded up, and its pixels are the same whatever the budget.
#[test]
fn a_step_draws_a_pictures_bands_within_its_budget() {
    let drawn = |k: usize| {
        let (mut s, sheet, _) = shaded_front([200.0, 150.0]);
        let mut answer = extract(&mut s, sheet).unwrap();
        while !answer.stale.is_empty() {
            step(&mut s);
            answer = extract(&mut s, sheet).unwrap();
        }
        let mut each = Vec::new();
        loop {
            let before = s.drawing_scenes.bands;
            let more = step_within(&mut s, &mut bands(k));
            each.push(s.drawing_scenes.bands - before);
            if !more {
                break;
            }
            assert!(each.len() < 100_000, "the picture never finished");
        }
        (each, only_picture(&extract(&mut s, sheet).unwrap()).rows.clone().unwrap())
    };
    let (one, rows) = drawn(1);
    let work: usize = one.iter().sum();
    assert!(work > 2 && one.iter().all(|n| *n == 1), "{one:?}");
    for k in [2, 3] {
        let (each, same) = drawn(k);
        assert_eq!(each.iter().sum::<usize>(), work, "the same bands, {k} a step");
        assert!(each.iter().all(|n| *n <= k), "never past its budget: {each:?}");
        assert_eq!(each.len(), work.div_ceil(k), "{each:?}");
        assert!(*same == *rows, "the same pixels");
    }
}

/// Once a shaded view's lines are drawn, `begin` hands on its picture's setup to do beside the
/// window, its triangles placed there too, and `step` then only draws its bands. A setup done for a
/// design changed since is dropped and asked for again; once one is taken, `begin` has nothing more,
/// and the picture is the one `step` alone draws.
#[test]
fn a_pictures_setup_is_a_job_beside_the_window() {
    let (mut s, sheet, _) = shaded_front([200.0, 150.0]);
    let mut answer = extract(&mut s, sheet).unwrap();
    while !answer.stale.is_empty() {
        let done = begin(&mut s).unwrap().run();
        assert!(finish(&mut s, done));
        answer = extract(&mut s, sheet).unwrap();
    }
    let job = begin(&mut s).expect("its picture's setup");
    assert!(matches!(job.0, Work::Picture(_)) && s.drawing_scenes.picturing.is_none());
    run(&mut s, "drawing.sheet.add", json!({}));
    let done = std::thread::spawn(move || job.run()).join().unwrap();
    assert!(!finish(&mut s, done) && s.drawing_scenes.picturing.is_none(), "set up for the design before");
    extract(&mut s, sheet).unwrap();
    let done = begin(&mut s).expect("asked for again").run();
    assert!(finish(&mut s, done) && s.drawing_scenes.picturing.as_ref().is_some_and(|p| !p.rows.preparing()), "its triangles placed");
    assert!(begin(&mut s).is_none(), "set up: its bands are step's");
    let rows = only_picture(&settled(&mut s, sheet)).rows.clone().unwrap();
    let (mut alone, sheet, _) = shaded_front([200.0, 150.0]);
    assert!(*only_picture(&settled(&mut alone, sheet)).rows.clone().unwrap() == *rows, "the pixels step alone draws");
}

/// A view of lines hands no picture, nothing waits for one, and no bodies are read for pictures.
#[test]
fn a_view_of_lines_hands_no_picture() {
    let (mut s, sheet, _) = front_and_top();
    let answer = settled(&mut s, sheet);
    assert!(answer.pictures.is_empty() && !answer.drawing_pictures && s.drawing_scenes.bodies.is_none());
}

/// A picture past the page's pixels is drawn at the sheet's dpi divided by the least whole number
/// that fits: at 2400 dpi, the 100 by 20 mm view is 17.9 million pixels, so it is drawn at 1200. The
/// grid is handed on before the picture is drawn, so its lines are all this needs.
#[test]
fn a_fine_sheet_draws_the_page_picture_coarser() {
    let (mut s, sheet, _) = shaded_front([200.0, 150.0]);
    run(&mut s, "drawing.sheet.set", json!({"sheet": ids::encode(sheet), "dpi": 2400}));
    let mut answer = extract(&mut s, sheet).unwrap();
    while !answer.stale.is_empty() {
        step(&mut s);
        answer = extract(&mut s, sheet).unwrap();
    }
    let picture = only_picture(&answer);
    assert!((picture.grid.pitch - 25.4 / 1200.0).abs() < 1e-12 && picture.grid.pixels() <= WINDOW_PICTURE_PIXELS, "{:?}", picture.grid);
    let rect = |side: f64, tall: f64| Picture { source: 1, rect: Aabb2 { min: Vec2::new(0.0, 0.0), max: Vec2::new(side, tall) }, opacity: 1.0 };
    for (dpi, k) in [(2400.0, 2.0), (1200.0, 1.0), (25.0, 1.0)] {
        assert_eq!(window_grid(&rect(100.0, 20.0), dpi).unwrap().pitch, 25.4 / (dpi / k), "{dpi}");
    }
    assert!(window_grid(&rect(5000.0, 5000.0), 2400.0).unwrap().pixels() <= WINDOW_PICTURE_PIXELS, "the largest paper at the finest dpi fits");
}

/// Only the drawing closed last keeps its pictures' pixels: once another closes, the first keeps its
/// lines but no pixels, so what is kept stays within one drawing's pictures.
#[test]
fn only_the_drawing_closed_last_keeps_its_pictures_pixels() {
    let (mut s, sheet, id) = shaded_front([200.0, 150.0]);
    let first = s.drawing("test").unwrap().id;
    settled(&mut s, sheet);
    run(&mut s, "drawing.close", json!({}));
    let pixels = |s: &Session| s.drawing_scenes.kept.of(first).and_then(|kept| kept.views.get(&id)).map(|view| view.pixels.len());
    assert_eq!(pixels(&s), Some(1), "kept at its close");
    run(&mut s, "drawing.create", json!({"name": "Second"}));
    let second = s.drawing("test").unwrap().sheets[0].id;
    extract(&mut s, second).unwrap();
    run(&mut s, "drawing.close", json!({}));
    assert_eq!(pixels(&s), Some(0), "its lines are kept, its pixels aren't");
}
