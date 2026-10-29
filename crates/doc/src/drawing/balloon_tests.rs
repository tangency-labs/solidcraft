use std::collections::BTreeSet;

use serde_json::{Value, json};
use solidcraft_geom::drawing_sheet::{self, Orientation, PaperSize};
use solidcraft_geom::drawing_view::NamedView;

use super::*;
use crate::drawing::Source;
use crate::drawing::view::{View, ViewKind, ViewPlace};
use crate::drawing_view::{ViewDirection, ViewOverrides};
use crate::format::DRAWING;
use crate::ids::IdSource;

const SEED: u64 = 3874;

fn edge(name: &str, path: Vec<u64>) -> ModelEdge {
    ModelEdge { body: "Bolt".into(), origin_feature: 4, name: name.into(), path }
}

/// A drawing of one design on one A3 sheet, its front view at (150, 150), and on the view two
/// balloons: one on a bolt placed straight in the design (occurrence 3), one on a bolt inside a
/// sub-assembly (occurrence 7 in 3). Their ids, in the order they were added.
fn ballooned() -> (Drawing, [u64; 2]) {
    let mut id_source = IdSource::seeded(SEED);
    let a3 = drawing_sheet::Sheet::standard(PaperSize::A3, Orientation::Landscape);
    let mut d = Drawing::new("Bolted", "en", "Sheet 1", a3, None, &BTreeSet::new(), &mut id_source).unwrap();
    let source = d.allocate_id(&mut id_source).unwrap();
    d.sources.push(Source { id: source });
    let (view, sheet) = (d.allocate_id(&mut id_source).unwrap(), d.sheets[0].id);
    let kind = ViewKind::Base { source, view: ViewDirection::Named(NamedView::Front) };
    d.add_views([Arc::new(View {
        id: view,
        sheet,
        kind,
        place: ViewPlace::At([150.0, 150.0]),
        settings: ViewOverrides { scale: Some("1:2".parse().unwrap()), ..ViewOverrides::default() },
        reference_margin_mm: 0.0,
    })]);
    let mut added = [0; 2];
    for ((path, at), id) in [(vec![3], [40.0, 25.0]), (vec![3, 7], [-35.5, 60.0])].into_iter().zip(&mut added) {
        *id = d.allocate_id(&mut id_source).unwrap();
        d.add_balloons([Arc::new(Balloon { id: *id, view, edge: edge("F1:side|F2:head", path), at })]);
    }
    d.validate().unwrap();
    (d, added)
}

fn file(d: &Drawing) -> Value {
    serde_json::from_str(&d.to_json().unwrap()).unwrap()
}

/// Balloon `id` in a drawing's file.
fn balloon<'a>(v: &'a mut Value, id: &str) -> &'a mut Value {
    v["balloons"].as_array_mut().unwrap().iter_mut().find(|x| x["id"] == id).unwrap()
}

/// Why `d`'s file, changed by `edit`, is refused, or "read".
fn refused(d: &Drawing, edit: impl FnOnce(&mut Value)) -> String {
    let mut v = file(d);
    edit(&mut v);
    Drawing::from_json(&v.to_string()).map_or_else(|e| e.to_string(), |_| "read".into())
}

/// A balloon saves its view, its edge by name with the part's occurrence path, and its place, and
/// nothing worked out. Balloons save by id and read back as saved, the same bytes again; a file
/// that lists them in another order reads by id. An older drawing reads with none, and a drawing
/// with none writes none.
#[test]
fn balloons_save_what_they_point_at_and_read_back_as_saved() {
    let (d, added) = ballooned();
    let [placed, nested] = added.map(ids::encode);
    let view = ids::encode(d.views[0].id);
    let mut v = file(&d);
    assert_eq!(
        *balloon(&mut v, &nested),
        json!({
            "id": nested, "view": view,
            "edge": {"body": "Bolt", "origin_feature": 4, "name": "F1:side|F2:head", "path": [3, 7]},
            "at": [-35.5, 60.0]
        })
    );
    assert_eq!(balloon(&mut v, &placed)["edge"]["path"], json!([3]));
    assert_eq!(d.balloon(added[1]).unwrap().occurrence(), [3, 7], "the part it numbers is its edge's path");
    let listed: Vec<u64> = v["balloons"].as_array().unwrap().iter().map(|x| ids::decode(x["id"].as_str().unwrap()).unwrap()).collect();
    let mut by_id = added.to_vec();
    by_id.sort_unstable();
    assert_eq!(listed, by_id);
    let reopened = Drawing::from_json(&d.to_json().unwrap()).unwrap();
    assert_eq!(reopened, d);
    assert_eq!(reopened.to_json().unwrap(), d.to_json().unwrap());
    let mut other_order = v.clone();
    other_order["balloons"].as_array_mut().unwrap().reverse();
    assert_eq!(Drawing::from_json(&other_order.to_string()).unwrap(), d);
    let mut unsorted = d.clone();
    unsorted.balloons.reverse();
    assert_eq!(unsorted.validate().unwrap_err().to_string(), "a drawing lists its balloons by id");
    let mut none = d.clone();
    none.balloons.clear();
    assert_eq!(file(&none).get("balloons"), None, "no balloons, none written");
    let older = json!({
        "format": DRAWING.format_string(1), "id": "0000000000002", "name": "Old", "sources": [], "language": "en",
        "sheets": [{"id": "0000000000001", "name": "S", "sheet": {"size": {"standard": "a4"}}}]
    });
    assert!(Drawing::from_json(&older.to_string()).unwrap().balloons.is_empty());
}

/// What reading refuses, each rule by name, on both sides of every cap: the view a balloon is on,
/// its edge's names, feature and path (a placed part's: 1 to 64 occurrences, none of them 0), its
/// place on paper, its id, how many there are, and fields it doesn't know.
#[test]
fn the_balloons_rules_are_kept_on_reading() {
    let (d, added) = ballooned();
    let [placed, nested] = added.map(ids::encode);
    let rule = |id: &str, rule: &str| format!("damaged drawing: balloon {id}: {rule}");
    let nothing = ids::encode(99);
    let on = format!("it is on view {nothing}, which the drawing doesn't have");
    assert_eq!(refused(&d, |v| balloon(v, &placed)["view"] = json!(nothing)), rule(&placed, &on));
    // The edge keeps a dimension's reference rules.
    let names = "a name in a reference is 1 to 512 bytes, without control characters";
    let body = |name: &str| refused(&d, |v| balloon(v, &nested)["edge"]["body"] = json!(name));
    let edge_name = |name: &str| refused(&d, |v| balloon(v, &nested)["edge"]["name"] = json!(name));
    for fits in ["x".repeat(512), "é".repeat(256)] {
        assert_eq!([body(&fits), edge_name(&fits)], ["read", "read"], "{} bytes", fits.len());
    }
    for broken in ["x".repeat(513), String::new(), "two\nlines".into()] {
        assert_eq!([body(&broken), edge_name(&broken)], [rule(&nested, names), rule(&nested, names)], "{} bytes", broken.len());
    }
    let feature = rule(&placed, "a reference names the feature that made its body");
    assert_eq!(refused(&d, |v| balloon(v, &placed)["edge"]["origin_feature"] = json!(0)), feature);
    // A placed part's path: 1 to 64 occurrences, none of them 0. A design's own body is no part.
    let path = |p: Value| refused(&d, |v| balloon(v, &nested)["edge"]["path"] = p);
    let deep = rule(&nested, "an occurrence path is at most 64 occurrences, none of them 0");
    let own = rule(&nested, "it points at a placed part: its edge's body is placed through an occurrence");
    assert_eq!(
        [path(json!([3])), path(json!(vec![3; 64])), path(json!(vec![3; 65])), path(json!([3, 0])), path(json!([]))],
        ["read".to_string(), "read".into(), deep.clone(), deep, own.clone()]
    );
    let no_path = refused(&d, |v| {
        balloon(v, &nested)["edge"].as_object_mut().unwrap().remove("path");
    });
    assert_eq!(no_path, own, "no path");
    // Its place is on paper.
    let paper = rule(&placed, "its place is at most 5000 mm on paper");
    let at = |at: [f64; 2]| refused(&d, |v| balloon(v, &placed)["at"] = json!(at));
    assert_eq!([at([5000.0, -5000.0]), at([5000.001, 0.0]), at([0.0, -5000.001])], ["read".to_string(), paper.clone(), paper]);
    assert!(refused(&d, |v| balloon(v, &placed)["at"] = json!([1.0])).starts_with("damaged drawing: "), "two numbers");
    // Its id is the drawing's: never another object's.
    let view_id = ids::encode(d.views[0].id);
    assert_eq!(refused(&d, |v| balloon(v, &placed)["id"] = json!(view_id)), format!("damaged drawing: two objects have the id {view_id}"));
    // Fields it doesn't know, at every level.
    let unknown: [fn(&mut Value, &str); 3] = [
        |v, id| balloon(v, id)["item"] = json!(3),
        |v, id| balloon(v, id)["edge"]["point"] = json!([0.0, 0.0, 0.0]),
        |v, id| balloon(v, id)["shape"] = json!("circle"),
    ];
    for (i, edit) in unknown.into_iter().enumerate() {
        assert!(refused(&d, |v| edit(v, &placed)).starts_with("damaged drawing: "), "{i}");
    }
    // At most MAX_BALLOONS balloons.
    let many = |n: usize| {
        let mut v = file(&d);
        let one = balloon(&mut v, &placed).clone();
        let copies = (1..=n as u64).map(|i| {
            let mut copy = one.clone();
            copy["id"] = json!(ids::encode(i));
            copy
        });
        v["balloons"] = Value::Array(copies.collect());
        Drawing::from_json(&v.to_string()).map(|d| d.balloons.len()).map_err(|e| e.to_string())
    };
    assert_eq!(many(MAX_BALLOONS), Ok(MAX_BALLOONS));
    assert_eq!(many(MAX_BALLOONS + 1), Err(format!("damaged drawing: a drawing has at most {MAX_BALLOONS} balloons")));
}

/// A sheet's balloons are its views' and go with it; a new id never takes a balloon's.
#[test]
fn a_sheets_balloons_go_with_it() {
    let (mut d, added) = ballooned();
    let sheet = d.sheets[0].id;
    let on = d.on_sheet(sheet);
    let mut by_id = added.to_vec();
    by_id.sort_unstable();
    assert_eq!(on.balloons, by_id);
    let mut id_source = IdSource::seeded(SEED);
    for _ in 0..64 {
        let id = d.allocate_id(&mut id_source).unwrap();
        assert!(!added.contains(&id), "a new id never takes a balloon's");
    }
    let second = d.allocate_id(&mut id_source).unwrap();
    d.sheets.push(Arc::new(crate::drawing::Sheet { id: second, name: "Sheet 2".into(), ..(*d.sheets[0]).clone() }));
    assert_eq!(d.remove_sheet(sheet), on);
    d.validate().unwrap();
    assert!(d.balloons.is_empty() && d.views.is_empty());
}
