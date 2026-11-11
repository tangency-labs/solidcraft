use std::f64::consts::TAU;

use serde_json::json;
use solidcraft_doc::drawing::symbol::Piece;
use solidcraft_geom::Seg2Geom;
use solidcraft_geom::paper::MAX_SIDE_MM;

use super::*;
use crate::schema::tests::conforms;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    let reply = s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"));
    let schema = output_schema_for::<SymbolResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &reply) || id != DEFINE, "{id}: {reply}");
    reply
}

fn refused(s: &mut Session, cmd: &str, p: Value) -> String {
    s.execute(cmd, &p).err().map(|e| e.to_string()).unwrap_or_else(|| panic!("accepted: {p}"))
}

fn saved(s: &Session) -> String {
    s.drawing("test").unwrap().to_json().unwrap()
}

/// A drawing of a box, open.
fn drawing() -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 80, "width": 40, "height": 30}));
    run(&mut s, "drawing.create", json!({}));
    s
}

fn line(a: [f64; 2], b: [f64; 2]) -> Value {
    json!({"type": "line", "a": a, "b": b})
}

/// A company mark: a square round a circle, a solid triangle, and the revision each placement gives.
fn mark() -> Value {
    let square = [[-5.0, -5.0], [5.0, -5.0], [5.0, 5.0], [-5.0, 5.0], [-5.0, -5.0]];
    json!([
        {"stroke": {"path": square.windows(2).map(|w| line(w[0], w[1])).collect::<Vec<_>>(), "closed": true, "line": "visible"}},
        {"stroke": {"path": [{"type": "arc", "center": [0, 0], "radius": 3, "start": 0, "sweep": TAU}], "closed": true, "line": "thin"}},
        {"fill": {"path": [line([-1.0, -1.0], [1.0, -1.0]), line([1.0, -1.0], [0.0, 1.0]), line([0.0, 1.0], [-1.0, -1.0])]}},
        {"text": {"at": [-4, -8], "content": [{"text": "Rev "}, {"field": {"source": "prompt", "key": "rev"}}], "cap_mm": 2.5, "align": "centre"}}
    ])
}

/// Defining, editing and deleting are one undo step each, an edit that changes nothing is none, and
/// the drawing lists its symbols with what each prompts for.
#[test]
fn a_symbol_is_defined_edited_and_deleted_in_one_step_each() {
    let mut s = drawing();
    let reply = run(&mut s, DEFINE, json!({"name": "Mark", "pieces": mark()}));
    let id = reply["symbol"].clone();
    assert_eq!((&reply["prompts"], &reply["record"]["name"]), (&json!(["rev"]), &json!("Mark")));
    assert_eq!(reply["record"]["pieces"][2]["fill"]["path"][0], line([-1.0, -1.0], [1.0, -1.0]));
    let listed = run(&mut s, "drawing.inspect", json!({}));
    assert_eq!(listed["symbols"], json!([{"id": id, "name": "Mark", "prompts": ["rev"]}]));

    let one_line = json!([{"stroke": {"path": [line([0.0, 0.0], [10.0, 0.0])], "line": "centre"}}]);
    let reply = run(&mut s, EDIT, json!({"symbol": id, "set": {"name": "Centre", "pieces": one_line}}));
    assert_eq!(reply["changed"], json!(["name", "pieces"]));
    let after = saved(&s);
    assert_eq!(run(&mut s, EDIT, json!({"symbol": id, "set": {"name": "Centre"}}))["changed"], json!([]));
    assert_eq!(saved(&s), after, "an edit that changes nothing changes nothing");
    assert_eq!(run(&mut s, "drawing.inspect", json!({}))["symbols"][0]["prompts"], json!([]));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.drawing("test").unwrap().symbols[0].name, "Mark", "one undo step");

    assert_eq!(run(&mut s, DELETE, json!({"symbol": id}))["symbol"], id);
    assert!(s.drawing("test").unwrap().symbols.is_empty());
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.drawing("test").unwrap().symbols.len(), 1);
    run(&mut s, "edit.undo", json!({}));
    assert!(s.drawing("test").unwrap().symbols.is_empty(), "the define is its own step");
}

#[test]
fn hostile_symbols_are_refused() {
    let mut s = drawing();
    let id = run(&mut s, DEFINE, json!({"name": "Mark", "pieces": mark()}))["symbol"].clone();
    run(&mut s, DEFINE, json!({"name": "Other", "pieces": mark()}));
    let before = saved(&s);
    let text = |content: Value| json!([{"text": {"at": [0, 0], "content": content}}]);
    let many = |n: usize, piece: &Value| Value::Array(vec![piece[0].clone(); n]);
    let cases = [
        (json!({"name": "Mark", "pieces": mark()}), "two symbols have one name"),
        (json!({"name": " ", "pieces": mark()}), "name is 1 to 256 bytes"),
        (json!({"name": "New", "pieces": []}), "1 to 1000 pieces"),
        (json!({"name": "New", "pieces": many(1001, &mark())}), "1 to 1000 pieces"),
        (json!({"name": "New", "pieces": many(65, &text(json!([{"text": "x"}])))}), "at most 64 of its pieces are texts"),
        (json!({"name": "New", "pieces": [{"fill": {"path": [line([0.0, 0.0], [1.0, 0.0])]}}]}), "ends where it starts"),
        (
            json!({"name": "New", "pieces": [{"stroke": {"path": [line([0.0, 0.0], [1.0, 0.0]), line([2.0, 0.0], [3.0, 0.0])], "line": "thin"}}]}),
            "meet",
        ),
        (json!({"name": "New", "pieces": [{"stroke": {"path": [line([0.0, 0.0], [5000.1, 0.0])], "line": "thin"}}]}), "5000 mm"),
        (json!({"name": "New", "pieces": [{"stroke": {"path": [line([0.0, 0.0], [1e300, 0.0])], "line": "thin"}}]}), "coordinate"),
        (json!({"name": "New", "pieces": [{"stroke": {"path": [], "line": "thin"}}]}), "at least one piece"),
        (json!({"name": "New", "pieces": [{"stroke": {"path": [line([0.0, 0.0], [1.0, 0.0])], "line": "wide"}}]}), "wide"),
        (json!({"name": "New", "pieces": text(json!([]))}), "some text"),
        (json!({"name": "New", "pieces": text(json!([{"field": {"source": "row", "key": "qty"}}]))}), "fields read"),
        (json!({"name": "New", "pieces": text(json!([{"text": "x".repeat(4097)}]))}), "4096"),
        (json!({"name": "New", "pieces": [{"text": {"at": [0, 0], "content": [{"text": "x"}], "cap_mm": 0.1}}]}), "height"),
        (json!({"name": "New", "pieces": [{"text": {"at": [6000, 0], "content": [{"text": "x"}]}}]}), "5000 mm"),
        (json!({"name": "New", "pieces": mark(), "origin": [0, 0]}), "origin"),
        (json!({"name": "New", "pieces": "x"}), "pieces"),
    ];
    for (p, says) in cases {
        let e = refused(&mut s, DEFINE, p.clone());
        assert!(e.contains(says), "{p}: {e}");
        assert_eq!(saved(&s), before, "{p}");
    }
    let nothing = ids::encode(99);
    for (cmd, p) in [(EDIT, json!({"symbol": nothing, "set": {}})), (DELETE, json!({"symbol": nothing}))] {
        let e = refused(&mut s, cmd, p);
        assert!(e.contains("no symbol"), "{cmd}: {e}");
    }
    let e = refused(&mut s, EDIT, json!({"symbol": id, "set": {"name": "Other"}}));
    assert!(e.contains("two symbols have one name"), "{e}");
    let e = refused(&mut s, EDIT, json!({"symbol": id, "set": {"pieces": []}}));
    assert!(e.contains("1 to 1000 pieces"), "{e}");
    let e = refused(&mut s, EDIT, json!({"symbol": id, "set": {"name": null}}));
    assert!(e.contains("name"), "{e}");
    assert_eq!(saved(&s), before);
}

/// The next of a seeded sequence (splitmix64).
fn splitmix(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// A coordinate on either side of the 5000 mm bound, or far past it.
fn coordinate(pick: &mut impl FnMut(usize) -> usize) -> f64 {
    const SIZES: [f64; 8] = [0.0, 1.0, -1.0, 4999.0, 5000.0, 5000.5, -6000.0, 1e6];
    SIZES[pick(SIZES.len())] * (pick(1001) as f64 / 1000.0)
}

/// Seeded random pieces, ends that meet or don't, arcs of every size and sweep, and texts anywhere:
/// never a panic, a refusal leaves the drawing as it was, and every point of a symbol kept is at
/// most 5000 mm from its origin.
#[test]
fn random_symbols_are_kept_on_paper_or_refused() {
    let mut s = drawing();
    let mut state = 13063u64;
    let mut pick = |n: usize| (splitmix(&mut state) % n as u64) as usize;
    let mut kept = 0;
    for round in 0..300 {
        let pieces: Vec<Value> = (0..round % 5)
            .map(|k| {
                let start = [coordinate(&mut pick), coordinate(&mut pick)];
                let end = if k % 2 == 0 { start } else { [coordinate(&mut pick), coordinate(&mut pick)] };
                let corner = [start[0] + 1.0, start[1]];
                match k % 3 {
                    0 => json!({"stroke": {"path": [line(start, corner), line(corner, end)], "closed": k % 2 == 0, "line": "visible"}}),
                    1 => {
                        let sweep = [0.0, 1.0, -TAU, TAU + 1.0, 1e-13][pick(5)];
                        let arc =
                            json!({"type": "arc", "center": start, "radius": coordinate(&mut pick), "start": coordinate(&mut pick), "sweep": sweep});
                        json!({"stroke": {"path": [arc], "line": "hidden"}})
                    }
                    _ => {
                        let cap = [0.5, 0.0, 100.0, 1e9][pick(4)];
                        json!({"text": {"at": start, "content": [{"text": "A"}], "cap_mm": cap}})
                    }
                }
            })
            .collect();
        let before = saved(&s);
        match s.execute(DEFINE, &json!({"name": format!("S{round}"), "pieces": pieces})) {
            Ok(_) => kept += 1,
            Err(e) => {
                assert!(!matches!(e, EngineError::Internal(..)), "round {round}: {e}");
                assert_eq!(saved(&s), before, "round {round}");
            }
        }
    }
    let d = s.drawing("test").unwrap();
    d.validate().unwrap();
    assert_eq!(d.symbols.len(), kept);
    assert!(kept > 0, "some random symbols are kept");
    for symbol in &d.symbols {
        for piece in &symbol.pieces {
            if let Piece::Stroke { path, .. } | Piece::Fill { path } = piece {
                let on_paper = |b: solidcraft_geom::Aabb2| [b.min.x, b.min.y, b.max.x, b.max.y].into_iter().all(|v| v.abs() <= MAX_SIDE_MM);
                assert!(path.iter().all(|seg| on_paper(seg.bounds())), "{}", symbol.name);
            }
        }
    }
}

// Placements (Y2a): where a symbol sits, anchored as a note is.

fn place_one(s: &mut Session, p: Value) -> Value {
    let reply = s.execute(PLACE, &p).unwrap_or_else(|e| panic!("{PLACE}: {e}"));
    let schema = output_schema_for::<PlacedResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &reply), "{reply}");
    reply
}

/// A box 80 x 40 x 30 mm as the part Bracket, and a drawing of it, open, with a front view at 1:1 at
/// (200, 150), and the mark of [`mark`] defined. The session, the view, the occurrence and the mark.
fn viewed() -> (Session, u64, u64, Value) {
    use solidcraft_doc::drawing::view::{View, ViewKind, ViewPlace};
    use solidcraft_doc::drawing_view::{ViewDirection, ViewOverrides};
    use solidcraft_geom::drawing_view::NamedView;
    let mut s = Session::default();
    let part = run(&mut s, "component.create", json!({"name": "Bracket"}))["occurrence"].as_u64().unwrap();
    run(&mut s, "solid.box", json!({"length": 80, "width": 40, "height": 30, "body_name": "Box"}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "drawing.create", json!({}));
    let mut d = (**s.drawing("test").unwrap()).clone();
    let id = d.allocate_id(&mut ids::IdSource::seeded(13063)).unwrap();
    let (sheet, source) = (d.sheets[0].id, d.sources[0].id);
    let kind = ViewKind::Base { source, view: ViewDirection::Named(NamedView::Front) };
    let settings = ViewOverrides { scale: Some("1:1".parse().unwrap()), ..ViewOverrides::default() };
    d.add_views([Arc::new(View { id, sheet, kind, place: ViewPlace::At([200.0, 150.0]), settings, reference_shown_percent: 0.0, label: None })]);
    d.validate().unwrap();
    s.doc_mut().put_drawing(Arc::new(d));
    let mark = run(&mut s, DEFINE, json!({"name": "Mark", "pieces": mark()}))["symbol"].clone();
    (s, id, part, mark)
}

/// Placing, editing and deleting a placement are one undo step each, an edit that changes nothing
/// is none, and the drawing lists its placements as saved, the angle in radians.
#[test]
fn a_symbol_is_placed_edited_and_deleted_in_one_step_each() {
    let (mut s, _, _, mark) = viewed();
    let first = json!({"symbol": mark, "anchor": {"sheet": "Sheet 1"}, "at": [40, 25], "scale": 2, "angle": 90, "answers": {"rev": "C"}});
    let reply = place_one(&mut s, first);
    let id = reply["placement"].clone();
    let record = &reply["record"];
    assert_eq!((&record["symbol"], &record["at"], &record["scale"]), (&mark, &json!([40.0, 25.0]), &json!(2.0)));
    assert_eq!((&record["angle"], &record["answers"]), (&json!(90f64.to_radians()), &json!({"rev": "C"})));
    let listed = run(&mut s, "drawing.inspect", json!({}));
    assert_eq!(listed["placed_symbols"], json!([record]));
    let plain = place_one(&mut s, json!({"symbol": mark, "anchor": {"sheet": "Sheet 1"}, "at": [0, 0]}));
    assert_eq!((&plain["record"]["scale"], &plain["record"]["angle"]), (&json!(1.0), &json!(0.0)), "a size of 1 and no turn unless given");
    assert!(plain["record"].get("answers").is_none(), "an unanswered prompt saves nothing: {plain}");

    let set = json!({"placement": id, "set": {"at": [50, 30], "scale": 0.5, "angle": -45, "answers": {}}});
    assert_eq!(run(&mut s, PLACED_EDIT, set)["changed"], json!(["at", "scale", "angle", "answers"]));
    let after = saved(&s);
    let same = json!({"placement": id, "set": {"at": [50, 30], "scale": 0.5, "angle": -45}});
    assert_eq!(run(&mut s, PLACED_EDIT, same)["changed"], json!([]));
    assert_eq!(saved(&s), after, "an edit that changes nothing changes nothing");
    run(&mut s, "edit.undo", json!({}));
    let placed = |s: &Session, id: &Value| {
        let id = ids::decode(id.as_str().unwrap()).unwrap();
        s.drawing("test").unwrap().placed_symbols.by_id(id).cloned()
    };
    assert_eq!(placed(&s, &id).unwrap().scale, 2.0, "one undo step");

    assert_eq!(run(&mut s, PLACED_DELETE, json!({"placement": id}))["placement"], id);
    assert!(placed(&s, &id).is_none());
    assert_eq!(s.drawing("test").unwrap().symbols.len(), 1, "its symbol stays");
    run(&mut s, "edit.undo", json!({}));
    assert!(placed(&s, &id).is_some(), "one undo step");
}

/// A placement sits on a view, or on an edge of the view's model; it goes with its view, is copied
/// with it, and is deleted as the drawing's other objects are.
#[test]
fn a_placement_goes_with_its_view_as_a_note_does() {
    let (mut s, view, part, mark) = viewed();
    let v = ids::encode(view);
    let on_view = place_one(&mut s, json!({"symbol": mark, "anchor": {"view": v}, "at": [-30, 20]}))["placement"].clone();
    let edge = json!({"edge": {"view": v, "edge": {"point": [40, 0, 0], "path": [part]}}});
    let on_edge = place_one(&mut s, json!({"symbol": mark, "anchor": edge, "at": [60, 40], "answers": {"rev": "B"}}))["placement"].clone();
    let saved_edge = &run(&mut s, "drawing.inspect", json!({}))["placed_symbols"];
    let edge_record = saved_edge.as_array().unwrap().iter().find(|p| p["id"] == on_edge).unwrap();
    assert_eq!(edge_record["anchor"]["edge"]["view"], json!(v));
    assert!(edge_record["anchor"]["edge"]["edge"]["name"].is_string(), "saved by name: {edge_record}");
    // Copied with its view, on the copy.
    let copy = run(&mut s, "drawing.view.copy_to_sheet", json!({"view": v, "sheet": "Sheet 1", "position_mm": [100, 100], "with_linked": false}));
    let copied = ids::decode(copy["view"].as_str().unwrap()).unwrap();
    let d = s.drawing("test").unwrap().clone();
    assert_eq!(d.placed_symbols.len(), 4);
    assert_eq!(d.placed_symbols.iter().filter(|p| p.anchor.view() == Some(copied)).count(), 2);
    run(&mut s, "edit.undo", json!({}));
    // Deleted by selection, and with its view.
    let item = |object: &Value| json!({"type": "drawing", "drawing": ids::encode(d.id), "object": object});
    let gone = run(&mut s, "selection.delete", json!({"items": [item(&on_view)]}));
    assert_eq!(gone["placed_symbols"], json!([on_view]));
    let gone = run(&mut s, "selection.delete", json!({"items": [item(&json!(v))]}));
    assert_eq!(gone["placed_symbols"], json!([on_edge]), "the view takes its placements");
    assert!(s.drawing("test").unwrap().placed_symbols.is_empty());
}

fn inspect_placed(s: &mut Session, placement: &Value) -> Value {
    let reply = s.execute(PLACED_INSPECT, &json!({"placement": placement})).unwrap_or_else(|e| panic!("{PLACED_INSPECT}: {e}"));
    let schema = output_schema_for::<PlacedInspectResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &reply), "{reply}");
    reply
}

/// Inspect gives where a placement's anchor stands with the model and what each of its texts says:
/// its prompts answered by the placement, unanswered ones empty, and a leader's end on the edge's
/// middle. It changes nothing.
#[test]
fn inspect_says_where_a_placement_stands_and_what_it_says() {
    let (mut s, view, part, mark) = viewed();
    let on_sheet = place_one(&mut s, json!({"symbol": mark, "anchor": {"sheet": "Sheet 1"}, "at": [40, 25], "answers": {"rev": "C"}}));
    let before = saved(&s);
    let reply = inspect_placed(&mut s, &on_sheet["placement"]);
    assert_eq!((&reply["status"], &reply["exact"], reply.get("point")), (&json!("ok"), &json!(true), None), "{reply}");
    assert_eq!(reply["record"], on_sheet["record"]);
    assert_eq!(reply["texts"], json!(["Rev C"]));
    assert_eq!(reply["fields"], json!([{"source": "prompt", "key": "rev", "state": "shown", "value": "C"}]));
    assert_eq!(saved(&s), before, "inspect changes nothing");

    let edge = json!({"edge": {"view": ids::encode(view), "edge": {"point": [40, 0, 0], "path": [part]}}});
    let on_edge = place_one(&mut s, json!({"symbol": mark, "anchor": edge, "at": [60, 40]}));
    let reply = inspect_placed(&mut s, &on_edge["placement"]);
    assert_eq!(reply["status"], json!("ok"), "{reply}");
    let point: Vec<f64> = reply["point"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
    assert!(point.iter().zip([40.0, 0.0, 0.0]).all(|(a, b)| (a - b).abs() < 1e-6), "the edge's middle: {point:?}");
    assert_eq!(reply["texts"], json!(["Rev "]), "an unanswered prompt shows nothing");
    assert_eq!(reply["fields"][0]["state"], json!("empty"), "{reply}");

    let unknown = refused(&mut s, PLACED_INSPECT, json!({"placement": ids::encode(9_999_999)}));
    assert!(unknown.contains("the drawing has no placed symbol"), "{unknown}");
}

/// A placed symbol's delete is refused, naming its placements, unless they go too: then both go in
/// one undo step.
#[test]
fn a_placed_symbol_is_deleted_only_with_its_placements() {
    let (mut s, _, _, mark) = viewed();
    let mut placements: Vec<Value> =
        (0..2).map(|i| place_one(&mut s, json!({"symbol": mark, "anchor": {"sheet": "Sheet 1"}, "at": [10 * i, 0]}))["placement"].clone()).collect();
    placements.sort_by_key(|p| ids::decode(p.as_str().unwrap()));
    let before = saved(&s);
    let why = refused(&mut s, DELETE, json!({"symbol": mark}));
    let named = placements.iter().map(|p| p.as_str().unwrap()).collect::<Vec<_>>().join(", ");
    assert!(why.contains(&format!("it is placed as {named}: give `\"placements\": \"delete\"`")), "{why}");
    assert_eq!(saved(&s), before, "a refusal changes nothing");
    assert!(refused(&mut s, DELETE, json!({"symbol": mark, "placements": "keep"})).contains("keep"));
    let reply = run(&mut s, DELETE, json!({"symbol": mark, "placements": "delete"}));
    assert_eq!(reply["placements"], json!(placements));
    let d = s.drawing("test").unwrap().clone();
    assert!(d.symbols.is_empty() && d.placed_symbols.is_empty());
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(saved(&s), before, "one undo step brings both back");
}

/// What placing refuses, on both sides of every bound, each refusal changing nothing.
#[test]
fn hostile_placements_are_refused() {
    let (mut s, view, part, mark) = viewed();
    let before = saved(&s);
    let at = |p: Value| {
        let mut base = json!({"symbol": mark, "anchor": {"sheet": "Sheet 1"}, "at": [0, 0]});
        base.as_object_mut().unwrap().extend(p.as_object().unwrap().clone());
        base
    };
    let cases = [
        (at(json!({"scale": 0.0099})), "its scale is 0.01 to 100"),
        (at(json!({"scale": 100.01})), "its scale is 0.01 to 100"),
        (at(json!({"scale": 0})), "its scale is 0.01 to 100"),
        (at(json!({"scale": -2})), "its scale is 0.01 to 100"),
        (at(json!({"answers": {"weld": "x"}})), "it answers `weld`, which symbol `Mark` doesn't prompt for"),
        (at(json!({"answers": {"rev": "a\nb"}})), "its answer to `rev`"),
        (at(json!({"at": [0, 5000.001]})), "its place is at most 5000 mm on paper"),
        (at(json!({"symbol": ids::encode(1)})), "the drawing has no symbol"),
        (at(json!({"anchor": {"view": ids::encode(1)}})), "the drawing has no view"),
        (at(json!({"anchor": {"sheet": "Sheet 9"}})), "Sheet 9"),
        (at(json!({"anchor": {"edge": {"view": ids::encode(view), "edge": {"point": [1e9, 0, 0], "path": [part]}}}})), ""),
        (at(json!({"angle": "ninety"})), ""),
        (at(json!({"leader": true})), "leader"),
    ];
    for (p, why) in cases {
        let got = refused(&mut s, PLACE, p.clone());
        assert!(got.contains(why), "{p}: {got}");
        assert_eq!(saved(&s), before, "{p}: a refusal changes nothing");
    }
    // Every finite angle is a turn, and the bounds themselves are kept.
    for p in [at(json!({"scale": 0.01})), at(json!({"scale": 100})), at(json!({"angle": 1e300})), at(json!({"angle": -720}))] {
        place_one(&mut s, p);
    }
    let id = place_one(&mut s, at(json!({})))["placement"].clone();
    let edited = |set: Value| json!({"placement": id, "set": set});
    assert!(refused(&mut s, PLACED_EDIT, edited(json!({"scale": 100.01}))).contains("its scale is 0.01 to 100"));
    assert!(refused(&mut s, PLACED_EDIT, edited(json!({"answers": {"weld": "x"}}))).contains("doesn't prompt for"));
    assert!(refused(&mut s, PLACED_EDIT, edited(json!({"symbol": mark}))).contains("symbol"), "what it places stays");
    assert!(refused(&mut s, PLACED_EDIT, json!({"placement": ids::encode(1), "set": {}})).contains("the drawing has no placed symbol"));
    assert!(refused(&mut s, PLACED_DELETE, json!({"placement": ids::encode(1)})).contains("the drawing has no placed symbol"));
}

/// Seeded random placements are kept and keep the drawing's rules, or are refused and change
/// nothing; never a panic.
#[test]
fn random_placements_are_kept_or_refused() {
    let (mut s, view, part, mark) = viewed();
    let mut state = 13063;
    let numbers = [json!(0), json!(0.01), json!(0.0099), json!(1), json!(100), json!(100.01), json!(-1), json!(1e300), json!(-1e-300)];
    let v = ids::encode(view);
    let anchors = [
        json!({"sheet": "Sheet 1"}),
        json!({"view": v}),
        json!({"edge": {"view": v, "edge": {"point": [40, 0, 0], "path": [part]}}}),
        json!({"corner": {"view": v, "corner": {"point": [80, 0, 30], "path": [part]}}}),
        json!({"view": ids::encode(1)}),
    ];
    let answers = [json!({}), json!({"rev": "A"}), json!({"rev": ""}), json!({"rev": "x\u{7}"}), json!({"other": "x"})];
    let (mut kept, mut refusals) = (0, 0);
    for _ in 0..300 {
        let mut pick = |n: usize| (splitmix(&mut state) % n as u64) as usize;
        let k = numbers.len();
        let (anchor, x, y, scale, angle, answer) = (pick(anchors.len()), pick(k), pick(k), pick(k), pick(k), pick(answers.len()));
        let p = json!({
            "symbol": mark,
            "anchor": anchors[anchor],
            "at": [numbers[x], numbers[y]],
            "scale": numbers[scale],
            "angle": numbers[angle],
            "answers": answers[answer],
        });
        let before = saved(&s);
        match s.execute(PLACE, &p) {
            Ok(_) => {
                s.drawing("test").unwrap().validate().unwrap();
                kept += 1;
            }
            Err(_) => {
                assert_eq!(saved(&s), before, "{p}");
                refusals += 1;
            }
        }
    }
    assert!(kept > 10 && refusals > 10, "{kept} kept, {refusals} refused");
}
