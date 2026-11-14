use std::collections::BTreeSet;
use std::f64::consts::{FRAC_PI_2, PI, TAU};
use std::sync::Arc;

use serde_json::{Value, json};
use solidcore_record::Vec2;
use solidcraft_geom::drawing_sheet::{self, Orientation, PaperSize};

use super::*;
use crate::format::DRAWING;
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
