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
