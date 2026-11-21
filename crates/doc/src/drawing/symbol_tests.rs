use std::collections::BTreeSet;
use std::f64::consts::{FRAC_PI_2, PI, TAU};
use std::sync::Arc;

use serde_json::{Value, json};
use solidcore_record::Vec2;
use solidcraft_geom::drawing_sheet::{self, Orientation, PaperSize};

use super::*;
use crate::format::{DRAWING, MAX_REAL};
use crate::ids::{self, IdSource};

const SEED: u64 = 13063;

fn line(a: [f64; 2], b: [f64; 2]) -> Seg2 {
    Seg2::Line { a: Vec2::new(a[0], a[1]), b: Vec2::new(b[0], b[1]) }
}

/// The closed path through `corners` in turn.
fn outline(corners: &[[f64; 2]]) -> Vec<Seg2> {
    corners.iter().zip(corners.iter().cycle().skip(1)).map(|(a, b)| line(*a, *b)).collect()
}

/// A company mark: a square round a circle, a solid triangle, and a revision letter each placement
/// gives.
fn mark(id: u64) -> Symbol {
    let rev = Content(vec![Part::Text("Rev ".into()), Part::Field { source: Source::Prompt, key: "rev".into(), format: Default::default() }]);
    Symbol {
        id,
        name: "Mark".into(),
        pieces: vec![
            Piece::Stroke { path: outline(&[[-5.0, -5.0], [5.0, -5.0], [5.0, 5.0], [-5.0, 5.0]]), closed: true, line: LineKind::Visible },
            Piece::Stroke { path: vec![Seg2::Arc { center: Vec2::ZERO, radius: 3.0, start: 0.0, sweep: TAU }], closed: true, line: LineKind::Thin },
            Piece::Fill { path: outline(&[[-1.0, -1.0], [1.0, -1.0], [0.0, 1.0]]) },
            Piece::Text { at: [-4.0, -8.0], content: rev, cap_mm: Some(2.5), align: HAlign::Centre },
        ],
    }
}

/// A drawing on one A3 sheet with two symbols, `Mark` and `Mark 2`, and their ids.
fn with_symbols() -> (Drawing, [u64; 2]) {
    let mut id_source = IdSource::seeded(SEED);
    let a3 = drawing_sheet::Sheet::standard(PaperSize::A3, Orientation::Landscape);
    let mut d = Drawing::new("Marked", "en", "Sheet 1", a3, None, &BTreeSet::new(), &mut id_source).unwrap();
    let mut added = [0; 2];
    for (name, id) in ["Mark", "Mark 2"].into_iter().zip(&mut added) {
        *id = d.allocate_id(&mut id_source).unwrap();
        d.symbols.add([Arc::new(Symbol { name: name.into(), ..mark(*id) })]);
    }
    d.validate().unwrap();
    (d, added)
}

fn file(d: &Drawing) -> Value {
    serde_json::from_str(&d.to_json().unwrap()).unwrap()
}

fn symbol(v: &mut Value, id: u64) -> &mut Value {
    let id = ids::encode(id);
    v["symbols"].as_array_mut().unwrap().iter_mut().find(|x| x["id"] == id).unwrap()
}

fn refused(d: &Drawing, edit: impl FnOnce(&mut Value)) -> String {
    let mut v = file(d);
    edit(&mut v);
    Drawing::from_json(&v.to_string()).map_or_else(|e| e.to_string(), |_| "read".into())
}

/// A symbol saves its name and pieces and reads back as saved, the same bytes again, in any listed
/// order. An older drawing reads with none, and a drawing with none writes none.
#[test]
fn symbols_save_what_they_are_and_read_back_as_saved() {
    let (d, [first, _]) = with_symbols();
    let mut v = file(&d);
    let saved = symbol(&mut v, first);
    assert_eq!(saved["name"], "Mark");
    assert_eq!(saved["pieces"][0]["stroke"]["path"][0], json!({"type": "line", "a": [-5.0, -5.0], "b": [5.0, -5.0]}));
    assert_eq!((&saved["pieces"][0]["stroke"]["closed"], &saved["pieces"][1]["stroke"]["line"]), (&json!(true), &json!("thin")));
    assert_eq!(
        saved["pieces"][3],
        json!({"text": {"at": [-4.0, -8.0], "content": [{"text": "Rev "}, {"field": {"source": "prompt", "key": "rev", "format": {"unit": null, "decimals": null}}}], "cap_mm": 2.5, "align": "centre"}})
    );
    let reopened = Drawing::from_json(&d.to_json().unwrap()).unwrap();
    assert_eq!(reopened, d);
    assert_eq!(reopened.to_json().unwrap(), d.to_json().unwrap());
    let mut other_order = v.clone();
    other_order["symbols"].as_array_mut().unwrap().reverse();
    assert_eq!(Drawing::from_json(&other_order.to_string()).unwrap(), d);
    let mut unsorted = d.clone();
    unsorted.symbols.reverse();
    assert_eq!(unsorted.validate().unwrap_err().to_string(), "a drawing lists its symbols by id");
    let mut none = d.clone();
    none.symbols.clear();
    assert_eq!(file(&none).get("symbols"), None);
    let older = json!({
        "format": DRAWING.format_string(1), "id": "0000000000002", "name": "Old", "sources": [], "language": "en",
        "sheets": [{"id": "0000000000001", "name": "S", "sheet": {"size": {"standard": "a4"}}}]
    });
    assert!(Drawing::from_json(&older.to_string()).unwrap().symbols.is_empty());
}

/// A symbol's prompts are its texts' prompt keys, each once and sorted; a symbol without texts has
/// none.
#[test]
fn a_symbols_prompts_are_what_its_texts_ask_for() {
    let mut s = mark(1);
    let asks =
        |keys: &[&str]| Content(keys.iter().map(|k| Part::Field { source: Source::Prompt, key: (*k).into(), format: Default::default() }).collect());
    s.pieces.push(Piece::Text { at: [0.0, 0.0], content: asks(&["weld", "rev"]), cap_mm: None, align: HAlign::Left });
    let sheet = Part::Field { source: Source::Sheet, key: "number".into(), format: Default::default() };
    s.pieces.push(Piece::Text { at: [0.0, 0.0], content: Content(vec![sheet]), cap_mm: None, align: HAlign::Left });
    assert_eq!(s.prompts().into_iter().collect::<Vec<_>>(), ["rev", "weld"]);
    s.pieces.retain(|piece| !matches!(piece, Piece::Text { .. }));
    assert!(s.prompts().is_empty());
}

/// What reading refuses, on both sides of every cap.
#[test]
fn the_symbols_rules_are_kept_on_reading() {
    let (d, [first, second]) = with_symbols();
    let rule = |rule: &str| format!("damaged drawing: symbol {}: {rule}", ids::encode(first));
    let pieces = |edit: &dyn Fn(&mut Value)| refused(&d, |v| edit(&mut symbol(v, first)["pieces"]));
    // Its name: as a sheet's, and its own.
    let name = |n: Value| refused(&d, |v| symbol(v, first)["name"] = n);
    let names = rule("its name is 1 to 256 bytes, not only spaces, without control characters");
    assert_eq!([name(json!(" ")), name(json!("x".repeat(257))), name(json!("a\nb"))], [names.clone(), names.clone(), names]);
    assert_eq!(name(json!("x".repeat(256))), "read");
    assert_eq!(name(json!("Mark 2")), "damaged drawing: two symbols have one name");
    // 1 to MAX_PIECES pieces, at most MAX_TEXTS of them texts.
    let count = |n: usize, at: usize| pieces(&|p| *p = Value::Array(vec![p[at].clone(); n]));
    assert_eq!([count(1, 0), count(MAX_PIECES, 0)], ["read", "read"]);
    let how_many = rule(&format!("it has 1 to {MAX_PIECES} pieces"));
    assert_eq!([count(0, 0), count(MAX_PIECES + 1, 0)], [how_many.clone(), how_many]);
    assert_eq!(count(MAX_TEXTS, 3), "read");
    assert_eq!(count(MAX_TEXTS + 1, 3), rule(&format!("at most {MAX_TEXTS} of its pieces are texts")));
    // Each path as a stroke on paper takes it: pieces that meet, and closed when it says so.
    let path = |at: usize, kind: &str, path: Vec<Seg2>| pieces(&|p| p[at][kind]["path"] = serde_json::to_value(&path).unwrap());
    let apart = vec![line([0.0, 0.0], [1.0, 0.0]), line([1.0 + 1e-6, 0.0], [2.0, 0.0])];
    assert_eq!(path(0, "stroke", apart), rule("a stroke's pieces meet end to start"));
    let open = vec![line([0.0, 0.0], [1.0, 0.0]), line([1.0, 0.0], [1.0, 1.0])];
    assert_eq!(path(0, "stroke", open.clone()), rule("a closed stroke ends where it starts"));
    assert_eq!(path(2, "fill", open.clone()), rule("a closed stroke ends where it starts"));
    assert_eq!(pieces(&|p| p[0]["stroke"] = json!({"path": serde_json::to_value(&open).unwrap(), "line": "hidden"})), "read");
    assert_eq!(path(0, "stroke", Vec::new()), rule("a stroke has at least one piece"));
    assert_eq!(path(1, "stroke", vec![Seg2::Arc { center: Vec2::ZERO, radius: 0.0, start: 0.0, sweep: TAU }]), rule("arc radius"));
    // Every point at most 5000 mm from the origin, an arc's far side too.
    let far = |x: f64| pieces(&|p| p[0]["stroke"] = json!({"path": [{"type": "line", "a": [0.0, 0.0], "b": [x, 0.0]}], "line": "visible"}));
    let paper = rule("each line is at most 5000 mm from its origin");
    assert_eq!([far(5000.0), far(5000.001), far(-5000.001)], ["read".to_string(), paper.clone(), paper.clone()]);
    let arc = |x: f64| path(1, "stroke", vec![Seg2::Arc { center: Vec2::new(x, 0.0), radius: 3.0, start: 0.0, sweep: TAU }]);
    assert_eq!([arc(4997.0), arc(4997.001)], ["read".to_string(), paper.clone()]);
    // A half turn whose ends are on paper and whose far side isn't.
    let bulge = Seg2::Arc { center: Vec2::new(4998.0, 0.0), radius: 3.0, start: -FRAC_PI_2, sweep: PI };
    assert_eq!(pieces(&|p| p[1]["stroke"] = json!({"path": [bulge], "line": "thin"})), paper);
    assert!(pieces(&|p| p[0]["stroke"]["line"] = json!("wide")).starts_with("damaged drawing: "));
    // A text: on paper, some words, the fields a symbol can read, a note's heights.
    let text = |key: &str, value: Value| pieces(&|p| p[3]["text"][key] = value.clone());
    let at = rule("a text starts at most 5000 mm from its origin");
    assert_eq!([text("at", json!([5000.0, -5000.0])), text("at", json!([0.0, 5000.001]))], ["read".to_string(), at]);
    assert_eq!(text("content", json!([])), rule("a text has some text"));
    let reads = rule("a text's fields read the drawing, its sheet, its component, its newest revision or a prompt");
    assert_eq!(text("content", json!([{"field": {"source": "row", "key": "qty"}}])), reads);
    assert_eq!(text("content", json!([{"field": {"source": "drawing", "key": "title"}}])), "read");
    assert!(text("content", json!([{"text": "x".repeat(4097)}])).starts_with(&rule("")));
    let cap = |mm: f64| text("cap_mm", json!(mm));
    let height = rule("a text's height is 0.5 to 100 mm");
    assert_eq!([cap(0.5), cap(100.0), cap(0.49), cap(100.1)], ["read".to_string(), "read".into(), height.clone(), height]);
    // Its id is the drawing's, and fields it doesn't know aren't read.
    let sheet = ids::encode(d.sheets[0].id);
    assert_eq!(refused(&d, |v| symbol(v, second)["id"] = json!(sheet)), format!("damaged drawing: two objects have the id {sheet}"));
    assert!(refused(&d, |v| symbol(v, first)["origin"] = json!([0, 0])).starts_with("damaged drawing: "));
    assert!(pieces(&|p| p[0] = json!({"picture": {}})).starts_with("damaged drawing: "));
    // At most MAX_SYMBOLS, each with a name of its own.
    let many = |n: usize| {
        let mut v = file(&d);
        let one = symbol(&mut v, first).clone();
        let copies = (1..=n as u64).map(|i| {
            let mut copy = one.clone();
            copy["id"] = json!(ids::encode(i));
            copy["name"] = json!(format!("Mark {i}"));
            copy
        });
        v["symbols"] = Value::Array(copies.collect());
        Drawing::from_json(&v.to_string()).map(|d| d.symbols.len()).map_err(|e| e.to_string())
    };
    assert_eq!(many(MAX_SYMBOLS), Ok(MAX_SYMBOLS));
    assert_eq!(many(MAX_SYMBOLS + 1), Err(format!("damaged drawing: a drawing has at most {MAX_SYMBOLS} symbols")));
}

/// A new id never takes a symbol's, and a sheet's delete leaves the symbols: they are the drawing's.
#[test]
fn symbols_belong_to_the_drawing_not_a_sheet() {
    let (mut d, added) = with_symbols();
    let mut id_source = IdSource::seeded(SEED);
    for _ in 0..64 {
        assert!(!added.contains(&d.allocate_id(&mut id_source).unwrap()), "a new id never takes a symbol's");
    }
    let (first, second) = (d.sheets[0].id, d.allocate_id(&mut id_source).unwrap());
    d.sheets.push(Arc::new(crate::drawing::Sheet { id: second, name: "Sheet 2".into(), ..(*d.sheets[0]).clone() }));
    d.remove_sheet(first);
    d.validate().unwrap();
    assert_eq!(d.symbols.iter().map(|s| s.id).collect::<Vec<_>>(), added);
}

/// Seeded mutations of a drawing's symbols never panic, and whatever reads again keeps the rules.
#[test]
fn mutated_symbol_files_read_or_are_refused() {
    let (d, _) = with_symbols();
    let base = file(&d);
    let values = [
        json!(null),
        json!(0),
        json!(-1.5),
        json!(1e308),
        json!("x"),
        json!([]),
        json!({}),
        json!([1.0, 2.0]),
        json!({"type": "arc", "center": [0.0, 0.0], "radius": 1e308, "start": 0.0, "sweep": 1.0}),
        json!({"type": "bezier", "n": 9, "p": []}),
    ];
    let pick = |seed: u64, salt: u64, n: usize| (((seed ^ salt).wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 33) as usize) % n;
    let spots = [
        "/name",
        "/pieces",
        "/pieces/0/stroke/path",
        "/pieces/0/stroke/path/1",
        "/pieces/0/stroke/path/1/b",
        "/pieces/1/stroke/path/0/sweep",
        "/pieces/2/fill/path",
        "/pieces/3/text/at",
    ];
    for seed in 1..=500u64 {
        let mut v = base.clone();
        let spot = format!("/symbols/{}{}", pick(seed, 1, 2), spots[pick(seed, 3, spots.len())]);
        *v.pointer_mut(&spot).unwrap() = values[pick(seed, 2, values.len())].clone();
        if let Ok(read) = Drawing::from_json(&v.to_string()) {
            read.validate().unwrap();
            assert_eq!(Drawing::from_json(&read.to_json().unwrap()).unwrap(), read, "seed {seed}");
        }
    }
}

// Placements (#13063 Y2a): where a symbol sits, anchored as a note is.

/// The drawing of [`with_symbols`] with a front view, and `Mark` placed three times: on the sheet
/// (answering its prompt), on the view, and on an edge in the view. Their ids, in that order, and
/// the view's.
fn placed() -> (Drawing, [u64; 3], u64) {
    use crate::drawing::Source as DrawingSource;
    use crate::drawing::dimension::ModelEdge;
    use crate::drawing::view::{View, ViewKind, ViewPlace};
    use crate::drawing_view::{ViewDirection, ViewOverrides};
    use solidcraft_geom::drawing_view::NamedView;
    let (mut d, [mark, _]) = with_symbols();
    let mut id_source = IdSource::seeded(SEED + 1);
    let source = d.allocate_id(&mut id_source).unwrap();
    d.sources.push(DrawingSource { id: source, path: None });
    let (view, sheet) = (d.allocate_id(&mut id_source).unwrap(), d.sheets[0].id);
    let kind = ViewKind::Base { source, view: ViewDirection::Named(NamedView::Front) };
    d.add_views([Arc::new(View {
        id: view,
        sheet,
        kind,
        place: ViewPlace::At([150.0, 150.0]),
        settings: ViewOverrides { scale: Some("1:2".parse().unwrap()), ..ViewOverrides::default() },
        reference_shown_percent: 0.0,
        label: None,
    })]);
    let edge = ModelEdge { body: "Bolt".into(), origin_feature: 4, name: "F1:side|F2:head".into(), path: vec![3] };
    let anchors = [Anchor::Sheet(sheet), Anchor::View(view), Anchor::Edge { view, edge }];
    let mut added = [0; 3];
    for (i, (anchor, id)) in anchors.into_iter().zip(&mut added).enumerate() {
        *id = d.allocate_id(&mut id_source).unwrap();
        let answers = if i == 0 { BTreeMap::from([("rev".to_string(), "C".to_string())]) } else { BTreeMap::new() };
        let place = PlacedSymbol { id: *id, symbol: mark, anchor, at: [40.0, 25.0], scale: 2.0, angle: FRAC_PI_2, answers };
        d.placed_symbols.add([Arc::new(place)]);
    }
    d.validate().unwrap();
    (d, added, view)
}

fn placement(v: &mut Value, id: u64) -> &mut Value {
    let id = ids::encode(id);
    v["placed_symbols"].as_array_mut().unwrap().iter_mut().find(|x| x["id"] == id).unwrap()
}

/// A placement saves its symbol by id, its anchor as a note's, its place, scale, angle and answers,
/// and reads back as saved. An older drawing reads with none, and one with none writes none.
#[test]
fn placements_save_where_a_symbol_sits_and_read_back_as_saved() {
    let (d, [on_sheet, on_view, _], view) = placed();
    let mut v = file(&d);
    let saved = placement(&mut v, on_sheet).clone();
    assert_eq!(saved["symbol"], json!(ids::encode(d.placed_symbols.by_id(on_sheet).unwrap().symbol)));
    assert_eq!(saved["anchor"], json!({"sheet": ids::encode(d.sheets[0].id)}));
    assert_eq!((&saved["at"], &saved["scale"], &saved["angle"]), (&json!([40.0, 25.0]), &json!(2.0), &json!(FRAC_PI_2)));
    assert_eq!(saved["answers"], json!({"rev": "C"}));
    let other = placement(&mut v, on_view).clone();
    assert_eq!(other["anchor"], json!({"view": ids::encode(view)}));
    assert!(other.get("answers").is_none(), "no answers, none saved: {other}");
    let read = Drawing::from_json(&v.to_string()).unwrap();
    assert_eq!(read.placed_symbols, d.placed_symbols);
    assert_eq!(read.to_json().unwrap(), d.to_json().unwrap(), "the same bytes again");
    let mut older = file(&d);
    older.as_object_mut().unwrap().remove("placed_symbols");
    assert!(Drawing::from_json(&older.to_string()).unwrap().placed_symbols.is_empty());
    let (bare, _) = with_symbols();
    assert!(file(&bare).get("placed_symbols").is_none());
    assert!(DRAWING.pending_names().iter().any(|name| name == "placed-symbols"), "{:?}", DRAWING.pending_names());
}

/// A drawing a newer build saved, with a field this build can't read, opens with its placements, as
/// with its symbols.
#[test]
fn a_newer_builds_drawing_opens_with_its_placements() {
    let (d, ..) = placed();
    let mut v = file(&d);
    v["format"] = json!(DRAWING.format_string(DRAWING.current() + 1));
    v["future_views"] = json!([{"id": "a"}]);
    let opened = Drawing::from_json(&v.to_string()).unwrap();
    assert_eq!(opened.newer_file.as_ref().unwrap().not_understood, ["field `future_views`"]);
    assert_eq!((&opened.symbols, &opened.placed_symbols), (&d.symbols, &d.placed_symbols));
}

/// An older build meets placements as one top-level field it doesn't know: everything they add sits
/// there, so it sets that field aside, opens the rest unchanged and saving writes it back.
#[test]
fn an_older_build_sets_placements_aside_and_reads_the_rest() {
    let (d, ..) = placed();
    let mut without = d.clone();
    without.placed_symbols.retain(|_| false);
    let mut v = file(&d);
    let placements = v.as_object_mut().unwrap().remove("placed_symbols").unwrap();
    assert_eq!(v, file(&without), "nothing else in the file changes");
    // As a build without them reads it: a newer format with a field it doesn't know.
    v["format"] = json!(DRAWING.format_string(DRAWING.current() + 1));
    v["unknown_placements"] = placements.clone();
    let opened = Drawing::from_json(&v.to_string()).unwrap();
    assert_eq!(opened.newer_file.as_ref().unwrap().not_understood, ["field `unknown_placements`"]);
    assert_eq!((&opened.sheets, &opened.symbols, &opened.views), (&d.sheets, &d.symbols, &d.views));
    let again: Value = serde_json::from_str(&opened.to_json().unwrap()).unwrap();
    assert_eq!(again["unknown_placements"], placements, "saving writes them back");
}

/// What reading refuses, on both sides of every bound.
#[test]
fn the_placements_rules_are_kept_on_reading() {
    let (d, [on_sheet, on_view, on_edge], _) = placed();
    let rule = |rule: &str| format!("damaged drawing: placed symbol {}: {rule}", ids::encode(on_sheet));
    let set = |key: &str, value: Value| refused(&d, |v| placement(v, on_sheet)[key] = value.clone());
    let scale = rule("its scale is 0.01 to 100");
    let sizes = [set("scale", json!(0.01)), set("scale", json!(100.0)), set("scale", json!(0.0099)), set("scale", json!(100.01))];
    assert_eq!(sizes, ["read".to_string(), "read".into(), scale.clone(), scale.clone()]);
    assert_eq!([set("scale", json!(0.0)), set("scale", json!(-1.0))], [scale.clone(), scale]);
    // Any angle the file's own number check lets through is read; past it, that check refuses it first.
    assert_eq!([set("angle", json!(-MAX_REAL)), set("angle", json!(MAX_REAL)), set("angle", json!(7.0 * TAU))], ["read", "read", "read"]);
    assert!(set("angle", json!(-1e300)).ends_with("is out of range"), "the file's own number check, before the record's");
    let place = rule("its place is at most 5000 mm on paper");
    assert_eq!([set("at", json!([5000.0, -5000.0])), set("at", json!([0.0, 5000.001]))], ["read".to_string(), place]);
    // Its symbol, sheet and view are the drawing's.
    let gone = ids::encode(1);
    assert_eq!(set("symbol", json!(gone)), rule(&format!("it places symbol {gone}, which the drawing doesn't have")));
    assert_eq!(set("anchor", json!({"sheet": gone})), rule(&format!("it is on sheet {gone}, which the drawing doesn't have")));
    assert_eq!(set("anchor", json!({"view": gone})), rule(&format!("it is on view {gone}, which the drawing doesn't have")));
    // Each answer names one of the definition's prompts and is one line.
    assert_eq!(set("answers", json!({"rev": "D", "weld": "x"})), rule("it answers `weld`, which symbol `Mark` doesn't prompt for"));
    assert_eq!(set("answers", json!({"rev": ""})), "read");
    assert_eq!(set("answers", json!({"rev": "x".repeat(4096)})), "read");
    let line = rule("its answer to `rev`: a label is one line of at most 4096 bytes");
    assert_eq!([set("answers", json!({"rev": "a\nb"})), set("answers", json!({"rev": "x".repeat(4097)}))], [line.clone(), line]);
    // An angle the file's own number check refuses on reading (NaN and infinity can't even be
    // written as JSON) is refused before it is saved, so a saved drawing always opens again.
    for angle in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, MAX_REAL * (1.0 + f64::EPSILON), -1e300] {
        let mut bad = d.clone();
        let at = bad.placed_symbols.index_of(on_view).unwrap();
        Arc::make_mut(bad.placed_symbols.get_mut(at).unwrap()).angle = angle;
        let why = bad.validate().unwrap_err().to_string();
        assert!(why.ends_with(&format!("its angle is at most {MAX_REAL} radians each way")), "{angle}: {why}");
    }
    // Its id is the drawing's, and fields it doesn't know aren't read.
    let edge_id = ids::encode(on_edge);
    assert_eq!(refused(&d, |v| placement(v, on_view)["id"] = json!(edge_id)), format!("damaged drawing: two objects have the id {edge_id}"));
    assert!(set("leader", json!(true)).starts_with("damaged drawing: "));
    // At most MAX_PLACEMENTS.
    let many = |n: usize| {
        let mut v = file(&d);
        let one = placement(&mut v, on_sheet).clone();
        let copies = (1..=n as u64).map(|i| {
            let mut copy = one.clone();
            copy["id"] = json!(ids::encode(i));
            copy
        });
        v["placed_symbols"] = Value::Array(copies.collect());
        Drawing::from_json(&v.to_string()).map(|d| d.placed_symbols.len()).map_err(|e| e.to_string())
    };
    assert_eq!(many(MAX_PLACEMENTS), Ok(MAX_PLACEMENTS));
    assert_eq!(many(MAX_PLACEMENTS + 1), Err(format!("damaged drawing: a drawing has at most {MAX_PLACEMENTS} placed symbols")));
}

/// A placement goes with its sheet, or with its view as a note does; a symbol's placements are
/// listed by id.
#[test]
fn a_sheets_and_a_views_placements_go_with_them() {
    let (d, [on_sheet, on_view, on_edge], view) = placed();
    let sheet = d.sheets[0].id;
    let by_id = |mut ids: Vec<u64>| {
        ids.sort_unstable();
        ids
    };
    assert_eq!(d.on_sheet(sheet).placed_symbols, by_id(vec![on_sheet, on_view, on_edge]));
    assert_eq!(d.with_contents(vec![view]).placed_symbols, by_id(vec![on_view, on_edge]));
    let mark = d.placed_symbols.by_id(on_sheet).unwrap().symbol;
    assert_eq!(d.placements_of(mark), by_id(vec![on_sheet, on_view, on_edge]));
    let other = d.symbols.iter().find(|s| s.id != mark).unwrap().id;
    assert!(d.placements_of(other).is_empty());
    assert!(d.has_object(on_edge));
    let mut without_view = d.clone();
    without_view.remove_objects(&without_view.with_contents(vec![view]));
    without_view.validate().unwrap();
    assert_eq!(without_view.placed_symbols.iter().map(|p| p.id).collect::<Vec<_>>(), [on_sheet]);
    let mut id_source = IdSource::seeded(SEED + 2);
    let mut two = d.clone();
    let second = two.allocate_id(&mut id_source).unwrap();
    two.sheets.push(Arc::new(crate::drawing::Sheet { id: second, name: "Sheet 2".into(), ..(*two.sheets[0]).clone() }));
    two.remove_sheet(sheet);
    two.validate().unwrap();
    assert!(two.placed_symbols.is_empty());
    assert!((0..64).all(|_| !d.has_object(d.allocate_id(&mut id_source).unwrap())), "a new id never takes a placement's");
}

/// Seeded mutations of a drawing's placements never panic, and whatever reads again keeps the rules.
#[test]
fn mutated_placement_files_read_or_are_refused() {
    let (d, _, _) = placed();
    let base = file(&d);
    let values = [
        json!(null),
        json!(0),
        json!(-1.5),
        json!(1e308),
        json!(0.0099),
        json!("x"),
        json!([]),
        json!({}),
        json!([1.0, 2.0]),
        json!({"rev": "a\u{7}"}),
        json!({"view": "0000000000001"}),
    ];
    let pick = |seed: u64, salt: u64, n: usize| (((seed ^ salt).wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 33) as usize) % n;
    let spots = ["/symbol", "/anchor", "/anchor/edge", "/at", "/at/1", "/scale", "/angle", "/answers", "/answers/rev"];
    for seed in 1..=500u64 {
        let mut v = base.clone();
        let spot = format!("/placed_symbols/{}{}", pick(seed, 1, 3), spots[pick(seed, 3, spots.len())]);
        let Some(at) = v.pointer_mut(&spot) else { continue };
        *at = values[pick(seed, 2, values.len())].clone();
        if let Ok(read) = Drawing::from_json(&v.to_string()) {
            read.validate().unwrap();
            assert_eq!(Drawing::from_json(&read.to_json().unwrap()).unwrap(), read, "seed {seed}");
        }
    }
}
