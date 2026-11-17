use serde_json::json;
use solidcore_record::Vec2;
use solidcraft_doc::drawing::cutting_line::{ArrowSide, CuttingLine, SectionKind};
use solidcraft_doc::drawing::view::{QuarterTurn, View, ViewPlace};
use solidcraft_doc::drawing_view::{ViewDirection, ViewOverrides};
use solidcraft_geom::drawing_view::NamedView;

use super::*;

/// Why an annotation is lost when a feature or body it names has no pair, for the paste's tests:
/// the reason stays private to the replace.
pub(crate) const NO_PAIR: &str = super::NO_PAIR;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"))
}

fn refused(s: &mut Session, p: Value) -> String {
    s.execute(REPLACE, &p).err().map(|e| e.to_string()).unwrap_or_else(|| panic!("accepted: {p}"))
}

fn saved(s: &Session) -> String {
    s.drawing("test").unwrap().to_json().unwrap()
}

/// Plate A, a component holding a plate 100 x 60 x 10 mm from the origin, called Plate, with a
/// 10 mm hole through its middle; plate B, A's Paste New copy 100 mm towards -Y, made 120 long;
/// then a 2 mm chamfer on A's top front edge, which B doesn't have. A drawing of them, open, with a
/// front view of A alone at 1:1.
pub(crate) struct Plates {
    pub(crate) s: Session,
    pub(crate) a: u64,
    pub(crate) b: u64,
    pub(crate) a_at: u64,
    pub(crate) b_at: u64,
    pub(crate) view: u64,
}

pub(crate) fn plates() -> Plates {
    let mut s = Session::default();
    let made = run(&mut s, "component.create", json!({"name": "Plate A"}));
    let (a, a_at) = (made["component"].as_u64().unwrap(), made["occurrence"].as_u64().unwrap());
    run(&mut s, "solid.box", json!({"length": 100, "width": 60, "height": 10, "body_name": "Plate"}));
    run(&mut s, "solid.cylinder", json!({"radius": 5, "height": 20, "base": [50, 30, -5], "operation": "cut", "targets": ["Plate"]}));
    run(&mut s, "component.activate", json!({"component": 0}));
    let copy = run(&mut s, "component.paste_new", json!({"component": a, "translate": [0, -100, 0]}));
    let (b, b_at) = (copy["component"].as_u64().unwrap(), copy["occurrence"].as_u64().unwrap());
    let b_box = s.doc.features.iter().find(|f| f.component == b && f.kind.base_name() == "Box").unwrap().id;
    run(&mut s, "timeline.edit", json!({"feature": b_box.to_string(), "set": {"length": 120}}));
    run(&mut s, "component.activate", json!({"component": a}));
    run(&mut s, "solid.chamfer", json!({"edges": [[50, 0, 10]], "distance": 2, "body": "Plate"}));
    run(&mut s, "component.activate", json!({"component": 0}));
    run(&mut s, "drawing.create", json!({}));
    let mut d = (**s.drawing("test").unwrap()).clone();
    let view = d.allocate_id(&mut ids::IdSource::seeded(13029)).unwrap();
    let (sheet, source) = (d.sheets[0].id, d.sources[0].id);
    let kind = ViewKind::Base { source, view: ViewDirection::Named(NamedView::Front) };
    let settings = ViewOverrides { scale: Some("1:1".parse().unwrap()), occurrences: Some(Arc::from(vec![vec![a_at]])), ..ViewOverrides::default() };
    d.add_views([Arc::new(View {
        id: view,
        sheet,
        kind,
        place: ViewPlace::At([200.0, 150.0]),
        settings,
        reference_shown_percent: 0.0,
        label: None,
    })]);
    d.validate().unwrap();
    s.doc_mut().put_drawing(Arc::new(d));
    Plates { s, a, b, a_at, b_at, view }
}

/// On A's front view: its bottom front edge's length (100), a note on the hole's top edge, a
/// balloon on its front right edge, and the length of the front edge the chamfer made (100). Their
/// ids, in that order.
pub(crate) fn annotated(p: &mut Plates) -> [Value; 4] {
    let (v, at) = (json!(ids::encode(p.view)), json!([p.a_at]));
    let length = |s: &mut Session, point: [f64; 3]| {
        run(s, "drawing.dimension.add", json!({"view": v, "kind": {"length": {"edge": {"point": point, "path": at}}}}))["dimension"].clone()
    };
    let width = length(&mut p.s, [50.0, 0.0, 0.0]);
    let anchor = json!({"edge": {"view": v, "edge": {"point": [55, 30, 10], "path": at}}});
    let note = run(&mut p.s, "drawing.note.add", json!({"anchor": anchor, "at": [60, 40], "content": "Through"}))["note"].clone();
    let balloon =
        run(&mut p.s, "drawing.balloon.add", json!({"view": v, "edge": {"point": [100, 0, 5], "path": at}, "at": [80, 20]}))["balloon"].clone();
    let chamfer = length(&mut p.s, [50.0, 0.0, 8.0]);
    [width, note, balloon, chamfer]
}

/// The issue's case: A's view made to show B. The width, the note and the balloon find their edges
/// on B and move onto it, and the width reads B's 120; the chamfer's edge names a feature B
/// doesn't have, so that dimension keeps its record, reads ok on A, is listed as not drawn and isn't
/// drawn. One undo step puts everything back.
#[test]
fn a_views_part_is_replaced_keeping_what_still_fits() {
    let mut p = plates();
    let [width, note, balloon, chamfer] = annotated(&mut p);
    let before = saved(&p.s);
    let v = ids::encode(p.view);
    let reply = run(&mut p.s, REPLACE, json!({"view": v, "to": {"component": p.b}}));
    let kept = json!([{"id": width, "kind": "dimension"}, {"id": note, "kind": "note"}, {"id": balloon, "kind": "balloon"}]);
    assert_eq!((&reply["views"], &reply["kept"], &reply["dry_run"]), (&json!([v]), &kept, &json!(false)), "{reply}");
    assert_eq!(reply["lost"], json!([{"id": chamfer, "kind": "dimension", "why": NO_PAIR}]), "{reply}");
    let read = run(&mut p.s, "drawing.dimension.inspect", json!({"dimension": width}));
    assert_eq!(read["status"], "ok", "{read}");
    assert!((read["value"].as_f64().unwrap() - 120.0).abs() < 1e-9, "{read}");
    let d = p.s.drawing("test").unwrap();
    assert_eq!(d.view(p.view).unwrap().settings.occurrences.as_deref(), Some(&[vec![p.b_at]][..]));
    assert_eq!(d.balloons[0].edge.path, [p.b_at]);
    let status = run(&mut p.s, "drawing.status", json!({}));
    let of = |id: &Value| status["dimensions"].as_array().unwrap().iter().find(|x| x["dimension"] == *id).unwrap().clone();
    let words = "its source draws only the occurrences it chose";
    assert_eq!((&of(&chamfer)["status"], &of(&chamfer)["not_drawn"]), (&json!("ok"), &json!(words)), "{status}");
    assert!(of(&width).get("not_drawn").is_none(), "{status}");
    let sheet = ids::encode(p.s.drawing("test").unwrap().sheets[0].id);
    let scene = run(&mut p.s, "drawing.scene", json!({"sheet": sheet}));
    let drawn = |id: &Value| scene["dimensions"].as_array().unwrap().iter().find(|x| x["dimension"] == *id).unwrap()["drawn"].clone();
    assert_eq!((drawn(&chamfer), drawn(&width)), (json!(false), json!(true)), "{scene}");
    run(&mut p.s, "edit.undo", json!({}));
    assert_eq!(saved(&p.s), before, "one undo step");
}

/// A note's branches move with it: one on the view alone, its one branch on the hole's top edge,
/// comes across whole, its branch on B; one on an edge with a branch on the chamfer's edge, which
/// B doesn't have, is lost whole, its anchor and branch both still on A, so no note is half moved.
#[test]
fn a_notes_branches_move_with_it() {
    let mut p = plates();
    let (v, at) = (json!(ids::encode(p.view)), json!([p.a_at]));
    let hole = json!({"edge": {"point": [55, 30, 10], "path": at}});
    let branched = json!({"anchor": {"view": v}, "at": [0, 0], "content": "B", "branches": [hole]});
    let branched = run(&mut p.s, "drawing.note.add", branched)["note"].clone();
    let anchor = json!({"edge": {"view": v, "edge": {"point": [50, 0, 0], "path": at}}});
    let chamfer = json!({"edge": {"point": [50, 0, 8], "path": at}});
    let half = json!({"anchor": anchor, "at": [0, 0], "content": "C", "branches": [chamfer]});
    let half = run(&mut p.s, "drawing.note.add", half)["note"].clone();
    let reply = run(&mut p.s, REPLACE, json!({"view": v, "to": {"component": p.b}}));
    assert_eq!(reply["kept"], json!([{"id": branched, "kind": "note"}]), "{reply}");
    assert_eq!(reply["lost"], json!([{"id": half, "kind": "note", "why": NO_PAIR}]), "{reply}");
    let d = p.s.drawing("test").unwrap();
    let note = |id: &Value| d.notes.by_id(ids::decode(id.as_str().unwrap()).unwrap()).unwrap().clone();
    let paths = |n: &Note| n.paths().map(<[u64]>::to_vec).collect::<Vec<_>>();
    assert_eq!(paths(&note(&branched)), [vec![p.b_at]]);
    assert_eq!(paths(&note(&half)), [vec![p.a_at], vec![p.a_at]]);
}

/// An annotation whose features pair but whose edge the new part doesn't have now is lost, and
/// says so. With B's hole suppressed, the note on its edge is lost, and so is a dimension from a
/// corner B still has to the hole's centre: every reference must find its one thing.
#[test]
fn an_annotation_whose_edge_the_new_part_lacks_is_lost() {
    let mut p = plates();
    let [width, note, ..] = annotated(&mut p);
    let (v, at) = (json!(ids::encode(p.view)), json!([p.a_at]));
    let kind = json!({"linear": {"a": {"point": [0, 60, 10], "path": at}, "b": {"centre": [55, 30, 10], "path": at}}});
    let centre = run(&mut p.s, "drawing.dimension.add", json!({"view": v, "kind": kind}))["dimension"].clone();
    let drawing = json!({"drawing": ids::encode(p.s.drawing("test").unwrap().id)});
    run(&mut p.s, "drawing.close", json!({}));
    let b_cut = p.s.doc.features.iter().find(|f| f.component == p.b && f.kind.base_name() == "Cylinder").unwrap().id;
    run(&mut p.s, "timeline.suppress", json!({"feature": b_cut, "suppressed": true}));
    run(&mut p.s, "drawing.open", drawing);
    let reply = run(&mut p.s, REPLACE, json!({"view": v, "to": {"component": p.b}}));
    let why = |id: &Value| reply["lost"].as_array().unwrap().iter().find(|x| x["id"] == *id).map(|x| x["why"].clone());
    assert_eq!((why(&note), why(&centre)), (Some(json!(NOT_FOUND)), Some(json!(NOT_FOUND))), "{reply}");
    assert!(reply["kept"].as_array().unwrap().iter().any(|x| x["id"] == width), "{reply}");
}

/// A dry run answers as the replace would and changes nothing: the same drawing, no undo step.
#[test]
fn a_dry_run_changes_nothing() {
    let mut p = plates();
    annotated(&mut p);
    let (before, undo) = (saved(&p.s), p.s.undo.len());
    let ask = |dry_run: bool| json!({"view": ids::encode(p.view), "to": {"component": p.b}, "dry_run": dry_run});
    let dry = run(&mut p.s, REPLACE, ask(true));
    assert_eq!((saved(&p.s), p.s.undo.len()), (before, undo));
    let done = run(&mut p.s, REPLACE, ask(false));
    for key in ["views", "kept", "lost", "dropped_hide", "sheets", "parts_lists"] {
        assert_eq!(dry[key], done[key], "{key}");
    }
    assert_eq!((&dry["dry_run"], p.s.undo.len()), (&json!(true), undo + 1));
}

/// The views projected from a replaced view follow it, and a hide path under the old part is
/// dropped from each view that sets one, and listed. The path is left from a part since deleted
/// from A: while A holds a part, the replace refuses it as more than one part.
#[test]
fn projected_views_follow_and_drop_their_hide_under_the_old_part() {
    let mut p = plates();
    let v = ids::encode(p.view);
    let projected = run(&mut p.s, "drawing.view.projected", json!({"parent": v, "side": "right"}))["view"].as_str().unwrap().to_string();
    // The model changes only while the drawing is closed.
    let drawing = json!({"drawing": ids::encode(p.s.drawing("test").unwrap().id)});
    let in_model = |s: &mut Session, change: &dyn Fn(&mut Session) -> Value| {
        run(s, "drawing.close", json!({}));
        let out = change(s);
        run(s, "drawing.open", drawing.clone());
        out
    };
    let pin = in_model(&mut p.s, &|s| {
        run(s, "component.activate", json!({"component": p.a}));
        let pin = run(s, "component.create", json!({"name": "Pin"}));
        run(s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [0, 0, 10], "operation": "new", "body_name": "Pin"}));
        run(s, "component.activate", json!({"component": 0}));
        pin
    });
    let under = [p.a_at, pin["occurrence"].as_u64().unwrap()];
    run(&mut p.s, "drawing.view.edit", json!({"view": projected, "set": {"hide": [under]}}));
    in_model(&mut p.s, &|s| run(s, "component.delete", json!({"component": pin["component"]})));
    let reply = run(&mut p.s, REPLACE, json!({"view": v, "to": {"component": p.b}}));
    let mut views: Vec<String> = serde_json::from_value(reply["views"].clone()).unwrap();
    views.sort();
    let mut want = vec![v.clone(), projected.clone()];
    want.sort();
    assert_eq!(views, want);
    assert_eq!(reply["dropped_hide"], json!([{"view": projected, "path": under}]), "{reply}");
    let d = p.s.drawing("test").unwrap();
    let id = ids::decode(&projected).unwrap();
    assert_eq!(d.view(id).unwrap().settings.hide, None);
    assert_eq!(d.resolve_view(id).unwrap().occurrences.value.as_deref(), Some(&[vec![p.b_at]][..]), "it follows its parent");
}

/// For a sheet, the component it documents and its parts lists follow the part too, and its title
/// block reads the new part's name.
#[test]
fn a_sheets_documented_part_and_parts_list_follow() {
    let mut p = plates();
    let source = ids::encode(p.s.drawing("test").unwrap().sources[0].id);
    run(&mut p.s, "drawing.sheet.set", json!({"sheet": "Sheet 1", "documents": {"source": source, "component": p.a}}));
    let list = run(&mut p.s, "drawing.table.add", json!({"sheet": "Sheet 1", "kind": "parts_list"}))["table"].clone();
    let title = |s: &mut Session| {
        let inspected = run(s, "drawing.inspect", json!({}));
        let cells = inspected["sheets"][0]["title_block_cells"].as_array().unwrap().clone();
        cells.iter().find(|c| c["name"] == "title").unwrap()["text"].clone()
    };
    assert_eq!(title(&mut p.s), "Plate A");
    let reply = run(&mut p.s, REPLACE, json!({"sheet": "Sheet 1", "to": {"component": p.b}}));
    let sheet = p.s.drawing("test").unwrap().sheets[0].clone();
    assert_eq!((&reply["sheets"], &reply["parts_lists"]), (&json!([ids::encode(sheet.id)]), &json!([list])), "{reply}");
    assert_eq!(sheet.documents.map(|d| d.component), Some(p.b));
    let b_name = p.s.doc.components.iter().find(|c| c.id == p.b).unwrap().name.clone();
    assert_eq!(title(&mut p.s), json!(b_name));
    let d = p.s.drawing("test").unwrap();
    let Kind::PartsList(of) = &d.tables[0].kind else { panic!("{:?}", d.tables[0].kind) };
    assert_eq!(of.of.component, p.b);
}

/// The pairs between A and B: A's box and cut pair with B's, and their bodies; A's chamfer, made
/// after the copy, has no pair. A part built the other way round (the cut first) pairs nothing,
/// and a part with itself pairs everything with itself.
/// A replace moves only what documented or listed the old part: a sheet that documents the design
/// itself and a parts list of the design stay as they are.
#[test]
fn a_sheet_and_a_parts_list_of_another_part_stay() {
    let mut p = plates();
    let source = ids::encode(p.s.drawing("test").unwrap().sources[0].id);
    run(&mut p.s, "drawing.sheet.set", json!({"sheet": "Sheet 1", "documents": {"source": source, "component": 0}}));
    let list = run(&mut p.s, "drawing.table.add", json!({"sheet": "Sheet 1", "kind": "parts_list"}))["table"].clone();
    let reply = run(&mut p.s, REPLACE, json!({"sheet": "Sheet 1", "to": {"component": p.b}}));
    assert_eq!((&reply["sheets"], &reply["parts_lists"]), (&json!([]), &json!([])), "{reply}");
    let d = p.s.drawing("test").unwrap();
    assert_eq!(d.sheets[0].documents.map(|c| c.component), Some(0));
    let Kind::PartsList(of) = &d.tables.by_id(ids::decode(list.as_str().unwrap()).unwrap()).unwrap().kind else { panic!("{list}") };
    assert_eq!(of.of.component, 0);
}

#[test]
fn features_pair_by_timeline_order_and_kind() {
    let mut p = plates();
    run(&mut p.s, "drawing.close", json!({}));
    let made = run(&mut p.s, "component.create", json!({"name": "Other"}));
    let other = made["component"].as_u64().unwrap();
    run(&mut p.s, "solid.cylinder", json!({"radius": 5, "height": 20, "base": [50, 30, -5], "body_name": "Pin"}));
    run(&mut p.s, "solid.box", json!({"length": 100, "width": 60, "height": 10, "operation": "new", "body_name": "Plate"}));
    run(&mut p.s, "component.activate", json!({"component": 0}));
    let state = p.s.model.state();
    let side = |component| Side { doc: &p.s.doc, state: &state, component };
    let own = |component: u64| p.s.doc.features.iter().filter(|f| f.component == component).map(|f| f.id).collect::<Vec<_>>();
    let (a, b) = (own(p.a), own(p.b));
    let pairs = FeaturePairs::between(side(p.a), side(p.b));
    assert_eq!(pairs.features, [(a[0], b[0]), (a[1], b[1])].into_iter().collect(), "the chamfer {} has no pair", a[2]);
    let b_body = state.bodies.iter().find(|x| x.feature == b[0]).unwrap().name.clone();
    assert_eq!(pairs.bodies, [("Plate".to_string(), b_body)].into_iter().collect());
    assert_eq!(FeaturePairs::between(side(p.a), side(other)), FeaturePairs::default());
    let same = FeaturePairs::between(side(p.a), side(p.a));
    assert_eq!(same.features, a.iter().map(|id| (*id, *id)).collect());
}

/// Refused, each with the drawing as it was: not exactly one of view, sheet and drawing; a view the
/// drawing doesn't have, or a projected one; a part the design doesn't place, or not where the path
/// says; the part the view shows already; and a view of more than one part.
#[test]
fn a_replace_is_refused_with_the_drawing_unchanged() {
    let mut p = plates();
    let v = json!(ids::encode(p.view));
    let projected = run(&mut p.s, "drawing.view.projected", json!({"parent": v, "side": "right"}))["view"].clone();
    let to = json!({"component": p.b});
    let mut d = (**p.s.drawing("test").unwrap()).clone();
    let whole = d.allocate_id(&mut ids::IdSource::seeded(1)).unwrap();
    let (sheet, source) = (d.sheets[0].id, d.sources[0].id);
    let kind = ViewKind::Base { source, view: ViewDirection::Named(NamedView::Top) };
    let settings = ViewOverrides { scale: Some("1:1".parse().unwrap()), ..ViewOverrides::default() };
    d.add_views([Arc::new(View {
        id: whole,
        sheet,
        kind,
        place: ViewPlace::At([100.0, 100.0]),
        settings,
        reference_shown_percent: 0.0,
        label: None,
    })]);
    // A section of the front view, and a flat pattern view: neither is a base view of a part.
    let (section, flat) = (d.allocate_id(&mut ids::IdSource::seeded(2)).unwrap(), d.allocate_id(&mut ids::IdSource::seeded(3)).unwrap());
    let line = CuttingLine::new(SectionKind::Full, vec![Vec2::new(-60.0, 0.0), Vec2::new(60.0, 0.0)], ArrowSide::Left).unwrap();
    let flat_kind = ViewKind::Flat { source, body: "Plate".into(), turn: QuarterTurn::None };
    let scaled = ViewOverrides { scale: Some("1:1".parse().unwrap()), ..ViewOverrides::default() };
    d.add_views([(section, ViewKind::Section { parent: p.view, line }, ViewOverrides::default()), (flat, flat_kind, scaled)].map(
        |(id, kind, settings)| {
            Arc::new(View { id, sheet, kind, place: ViewPlace::At([300.0, 100.0]), settings, reference_shown_percent: 0.0, label: None })
        },
    ));
    p.s.doc_mut().put_drawing(Arc::new(d));
    let before = saved(&p.s);
    let cases = [
        (json!({"view": v, "sheet": "Sheet 1", "to": to}), "give exactly one of view, sheet and drawing: true"),
        (json!({"to": to}), "give exactly one of view, sheet and drawing: true"),
        (json!({"drawing": false, "to": to}), "give exactly one of view, sheet and drawing: true"),
        (json!({"view": ids::encode(99), "to": to}), "the drawing has no view"),
        (json!({"view": projected, "to": to}), "name its base view"),
        (json!({"view": ids::encode(section), "to": to}), "is a section view of view"),
        (json!({"view": ids::encode(flat), "to": to}), "not supported yet: view"),
        (json!({"view": v, "to": {"component": 999}}), "the design has no placed component 999"),
        (json!({"view": v, "to": {"component": p.b, "path": [p.a_at]}}), "doesn't place component"),
        (json!({"view": v, "to": {"component": p.a}}), "the views show that part already"),
        (json!({"view": ids::encode(whole), "to": to}), "not supported yet: view"),
        (json!({"sheet": "Sheet 1", "to": to}), "not supported yet: view"),
        (json!({"view": v, "to": to, "dry_run": "yes"}), "dry_run"),
    ];
    for (params, words) in cases {
        let e = refused(&mut p.s, params.clone());
        assert!(e.contains(words), "{params}: {e}");
        assert_eq!(saved(&p.s), before, "{params}");
    }
}

/// A hostile value in each place of the parameters, at every depth: never a panic, and the drawing
/// keeps its rules.
#[test]
fn hostile_replace_params_never_panic() {
    let mut p = plates();
    annotated(&mut p);
    let template = json!({"view": ids::encode(p.view), "to": {"component": p.b, "path": [p.b_at]}, "dry_run": false});
    let mut at = Vec::new();
    crate::tests::json_places(&template, String::new(), &mut at);
    let mut values = crate::tests::hostile_values();
    values.extend([json!(u64::MAX), Value::Array(vec![json!(1); 1000]), json!("x".repeat(600)), json!({"component": 0})]);
    let mut internal = Vec::new();
    for (place, value) in at.iter().flat_map(|place| values.iter().map(move |value| (place, value))) {
        let mut params = template.clone();
        if let Some(slot) = params.pointer_mut(place) {
            *slot = value.clone();
        }
        let mut copy = p.s.scratch();
        if let Err(crate::EngineError::Internal(_, msg)) = copy.execute(REPLACE, &params) {
            internal.push(format!("{params}: {msg}"));
        }
        copy.doc.drawings.iter().for_each(|d| d.validate().unwrap());
    }
    assert!(internal.is_empty(), "drawing.source.replace panicked:\n{}", internal.join("\n"));
}
