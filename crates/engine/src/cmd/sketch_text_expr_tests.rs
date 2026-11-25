//! A sketch text's height, place and angle can be expressions of the design's parameters: the
//! text keeps the expression, follows the parameters, and a number typed instead replaces it.

use serde_json::{Value, json};
use solidcore_record::Vec2;
use solidcraft_geom::prelude::*;
use solidcraft_sketch::LinkSource;

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

/// Lower-left and upper-right corners of the outlines of the text in sketch `S`.
fn text_box(s: &Session) -> (Vec2, Vec2) {
    let id = s.doc.find_feature("S").unwrap().id;
    let st = s.model.state();
    let sk = &st.sketch(id).unwrap().sketch;
    let (mut lo, mut hi) = (Vec2::new(f64::MAX, f64::MAX), Vec2::new(f64::MIN, f64::MIN));
    for q in sk.points.iter().filter(|q| q.link.is_some()) {
        lo = Vec2::new(lo.x.min(q.pos.x), lo.y.min(q.pos.y));
        hi = Vec2::new(hi.x.max(q.pos.x), hi.y.max(q.pos.y));
    }
    (lo, hi)
}

fn no_trouble(s: &Session) {
    for r in &s.model.results {
        assert!(r.error.is_none() && r.warning.is_none(), "{}: {:?} {:?}", r.name, r.error, r.warning);
    }
}

/// The numbers and expressions stored for the text `link` of sketch `S`.
fn stored(s: &Session, link: &str) -> (Vec2, f64, f64, solidcraft_sketch::TextExprs) {
    let id = s.doc.find_feature("S").unwrap().id;
    let LinkSource::Text { at, height, angle, exprs, .. } = &s.doc.sketch(id).unwrap().link(link).unwrap().source else { panic!("not a text") };
    (*at, *height, *angle, exprs.clone())
}

fn reopened(s: &Session) -> Session {
    Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap())
}

#[test]
fn a_texts_height_and_place_typed_as_expressions_follow_their_parameters() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "tall", "expression": "20 mm"}));
    run(&mut s, "parameters.add", json!({"name": "left", "expression": "30 mm"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
    // Capital letters without descenders: the outlines stand on the baseline, one height tall.
    let r = run(&mut s, "sketch.text", json!({"text": "LIVE", "at": ["left", 25], "height": "tall"}));
    let link = r["link"].as_str().unwrap().to_string();
    assert_eq!(r["exprs"], json!({"x": "left", "height": "tall"}), "{r}");
    run(&mut s, "sketch.finish", json!({}));
    no_trouble(&s);
    let (lo, hi) = text_box(&s);
    assert!((lo.y - 25.0).abs() < 1e-9 && (hi.y - 45.0).abs() < 1e-6, "{lo:?} {hi:?}");
    // The first letter's side bearing is a fixed share of the height.
    let bearing = (lo.x - 30.0) / 20.0;
    assert!(bearing > 0.0 && bearing < 0.5, "{lo:?}");

    // Smaller and further left: the outlines follow both parameters, and so do they after the
    // design is saved and opened again.
    run(&mut s, "parameters.change", json!({"name": "tall", "expression": "8 mm"}));
    run(&mut s, "parameters.change", json!({"name": "left", "expression": "5 mm"}));
    let check = |s: &Session, left: f64, tall: f64| {
        no_trouble(s);
        let (lo, hi) = text_box(s);
        assert!((lo.y - 25.0).abs() < 1e-9 && (hi.y - 25.0 - tall).abs() < 1e-6, "{lo:?} {hi:?}");
        assert!((lo.x - left - bearing * tall).abs() < 1e-6, "{lo:?}");
    };
    check(&s, 5.0, 8.0);
    let json = s.doc.to_json();
    assert!(json.contains("\"exprs\""), "the design file keeps the expressions");
    check(&reopened(&s), 5.0, 8.0);
    let (_, _, _, exprs) = stored(&s, &link);
    assert_eq!((exprs.x.as_deref(), exprs.y, exprs.height.as_deref(), exprs.angle), (Some("left"), None, Some("tall"), None));

    // Editing the sketch afterwards keeps the text where its parameters put it.
    run(&mut s, "sketch.edit", json!({"sketch": "S"}));
    run(&mut s, "sketch.point", json!({"point": [200, 200]}));
    run(&mut s, "sketch.edit_text", json!({"link": link, "text": "LIVE"}));
    run(&mut s, "sketch.finish", json!({}));
    check(&s, 5.0, 8.0);
    let (at, height, _, exprs) = stored(&s, &link);
    assert!((at.x - 5.0).abs() < 1e-9 && (height - 8.0).abs() < 1e-9 && exprs.height.as_deref() == Some("tall"), "{at:?} {height} {exprs:?}");
}

#[test]
fn a_texts_angle_can_be_an_expression_too() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "turn", "expression": "0 deg"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
    let r = run(&mut s, "sketch.text", json!({"text": "LIVE", "at": [0, 0], "height": 10, "angle": "turn"}));
    let link = r["link"].as_str().unwrap().to_string();
    assert_eq!(r["exprs"], json!({"angle": "turn"}), "{r}");
    run(&mut s, "sketch.finish", json!({}));
    let (lo, hi) = text_box(&s);
    assert!(hi.x - lo.x > hi.y - lo.y, "along the x axis: {lo:?} {hi:?}");
    run(&mut s, "parameters.change", json!({"name": "turn", "expression": "90 deg"}));
    no_trouble(&s);
    let (lo, hi) = text_box(&s);
    assert!(hi.y - lo.y > hi.x - lo.x && hi.x.abs() < 1e-9 && lo.x < -9.0, "upright, the letters run up the y axis: {lo:?} {hi:?}");
    assert_eq!(stored(&s, &link).3.angle.as_deref(), Some("turn"));
    // Reopened, the text stands as the parameter says.
    let (lo, hi) = text_box(&reopened(&s));
    assert!(hi.y - lo.y > hi.x - lo.x, "{lo:?} {hi:?}");
}

#[test]
fn the_parameters_a_text_uses_are_listed_renamed_and_kept() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "tall", "expression": "12 mm"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
    let r = run(&mut s, "sketch.text", json!({"text": "LIVE", "at": [0, 0], "height": "tall * 2"}));
    let link = r["link"].as_str().unwrap().to_string();
    run(&mut s, "sketch.finish", json!({}));
    let users = run(&mut s, "parameters.users", json!({"name": "tall"}));
    assert!(users.to_string().contains("Text height"), "{users}");
    assert!(users["drives"].to_string().contains("\"S\""), "{users}");
    let e = s.execute("parameters.delete", &json!({"name": "tall"})).unwrap_err().to_string();
    assert!(e.contains("Text height"), "{e}");
    run(&mut s, "parameters.rename", json!({"name": "tall", "new_name": "big"}));
    assert_eq!(stored(&s, &link).3.height.as_deref(), Some("big * 2"));
    no_trouble(&s);
    let (lo, hi) = text_box(&s);
    assert!((hi.y - lo.y - 24.0).abs() < 1e-6, "{lo:?} {hi:?}");
    run(&mut s, "parameters.change", json!({"name": "big", "expression": "5 mm"}));
    let (lo, hi) = text_box(&s);
    assert!((hi.y - lo.y - 10.0).abs() < 1e-6, "{lo:?} {hi:?}");
    // The edges of the dependency graph name it, too.
    let graph = run(&mut s, "parameters.graph", json!({}));
    assert!(graph["edges"].to_string().contains("big"), "{graph}");
}

#[test]
fn a_number_replaces_an_expression_and_plain_numbers_save_as_before() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "tall", "expression": "20 mm"}));
    run(&mut s, "parameters.add", json!({"name": "left", "expression": "30 mm"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
    // Numbers, and expressions naming no parameter, are numbers: nothing new is saved.
    let r = run(&mut s, "sketch.text", json!({"text": "A", "at": [3, "2 mm + 2 mm"], "height": "10 mm", "angle": "30 deg"}));
    let plain = r["link"].as_str().unwrap().to_string();
    assert!(r.get("exprs").is_none(), "{r}");
    let (at, height, angle, exprs) = stored(&s, &plain);
    assert!(at.dist(Vec2::new(3.0, 4.0)) < 1e-12 && (height - 10.0).abs() < 1e-12 && (angle - 30f64.to_radians()).abs() < 1e-12, "{at:?}");
    assert!(exprs.is_empty());
    assert!(!s.doc.to_json().contains("exprs"), "a design without expressions saves as before");

    let r = run(&mut s, "sketch.text", json!({"text": "LIVE", "at": ["left", "left / 2"], "height": "tall"}));
    let link = r["link"].as_str().unwrap().to_string();
    // What an edit leaves out stays an expression; a number given replaces what it was.
    run(&mut s, "sketch.edit_text", json!({"link": link, "text": "LIVE!"}));
    assert_eq!(stored(&s, &link).3.height.as_deref(), Some("tall"));
    let r = run(&mut s, "sketch.edit_text", json!({"link": link, "height": 12}));
    assert_eq!(r["exprs"], json!({"x": "left", "y": "left / 2"}), "{r}");
    let (at, height, _, exprs) = stored(&s, &link);
    assert!((height - 12.0).abs() < 1e-12 && exprs.height.is_none() && at.dist(Vec2::new(30.0, 15.0)) < 1e-9, "{at:?} {height} {exprs:?}");
    run(&mut s, "sketch.edit_text", json!({"link": link, "at": [7, "left"]}));
    assert_eq!(run(&mut s, "sketch.edit_text", json!({"link": link}))["exprs"], json!({"y": "left"}));
    // Moving the text turns what moves into numbers; the height and the other coordinate stay.
    run(&mut s, "sketch.edit_text", json!({"link": link, "at": ["left", "left"], "height": "tall"}));
    let curve = s.doc.sketch(s.active_sketch.unwrap()).unwrap().link_curves(&link)[0];
    let curve = s.doc.sketch(s.active_sketch.unwrap()).unwrap().curves[curve].id.clone();
    run(&mut s, "sketch.move", json!({"entities": [curve], "translate": [10, 0]}));
    let (at, _, _, exprs) = stored(&s, &link);
    assert!((at.x - 40.0).abs() < 1e-9 && (at.y - 30.0).abs() < 1e-9, "{at:?}");
    assert_eq!((exprs.x, exprs.y.as_deref(), exprs.height.as_deref()), (None, Some("left"), Some("tall")));
}

#[test]
fn a_text_expression_that_cannot_be_used_is_refused_and_changes_nothing() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "turn", "expression": "30 deg"}));
    run(&mut s, "parameters.add", json!({"name": "tall", "expression": "20 mm"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
    let r = run(&mut s, "sketch.text", json!({"text": "A", "at": [0, 0]}));
    let link = r["link"].as_str().unwrap().to_string();
    let before = s.doc.to_json();
    let undo = s.undo.len();
    for (extra, error) in [
        (json!({"at": ["nope", 0]}), "`at` x"),
        (json!({"at": [0, "turn"]}), "`at` y"),
        (json!({"at": [0, "tall +"]}), "`at` y"),
        (json!({"height": "nope"}), "unknown parameter"),
        (json!({"height": "turn"}), "`height`"),
        (json!({"height": "tall - 30 mm"}), "`height` must be positive"),
        (json!({"height": "-3 mm"}), "`height` must be positive"),
        (json!({"height": "0 * tall"}), "`height` must be positive"),
        (json!({"height": ""}), "`height` must be a length"),
        (json!({"angle": "tall"}), "`angle`"),
        (json!({"angle": "5 mm"}), "`angle`"),
        (json!({"angle": "nope"}), "unknown parameter"),
    ] {
        let mut create = json!({"text": "A", "at": [0, 0]});
        let mut edit = json!({"link": link});
        for (key, value) in extra.as_object().unwrap() {
            create[key] = value.clone();
            edit[key] = value.clone();
        }
        let e = s.execute("sketch.text", &create).unwrap_err().to_string();
        assert!(e.contains(error), "{create}: {e}");
        for live in [false, true] {
            edit["live"] = json!(live);
            let e = s.execute("sketch.edit_text", &edit).unwrap_err().to_string();
            assert!(e.contains(error), "{edit}: {e}");
        }
        assert_eq!(s.doc.to_json(), before, "{extra}");
        assert_eq!(s.undo.len(), undo, "{extra}");
    }
}

#[test]
fn a_parameter_that_leaves_a_text_no_height_is_an_error_of_its_sketch_not_a_crash() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "tall", "expression": "12 mm"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
    run(&mut s, "sketch.text", json!({"text": "LIVE", "at": [0, 0], "height": "tall"}));
    run(&mut s, "sketch.finish", json!({}));
    no_trouble(&s);
    run(&mut s, "parameters.change", json!({"name": "tall", "expression": "0 mm"}));
    let sketch = s.model.results.iter().find(|r| r.name == "S").unwrap();
    assert!(sketch.error.as_deref().is_some_and(|e| e.contains("height")), "{:?}", sketch.error);
    run(&mut s, "parameters.change", json!({"name": "tall", "expression": "6 mm"}));
    no_trouble(&s);
    let (lo, hi) = text_box(&s);
    assert!((hi.y - lo.y - 6.0).abs() < 1e-6, "{lo:?} {hi:?}");
}

/// The height of the outlines of the texts in the sketch feature `id`.
fn outline_height(s: &Session, id: u64) -> f64 {
    let st = s.model.state();
    let sk = &st.sketch(id).unwrap().sketch;
    let ys: Vec<f64> = sk.points.iter().filter(|q| q.link.is_some()).map(|q| q.pos.y).collect();
    ys.iter().cloned().fold(f64::MIN, f64::max) - ys.iter().cloned().fold(f64::MAX, f64::min)
}

/// The first dimension's parameter of the sketch feature `id` and the expression of its text's height.
fn dimension_and_expression(s: &Session, id: u64) -> (String, Option<String>) {
    let sk = s.doc.sketch(id).unwrap();
    let dimension = sk.constraints.iter().find_map(|c| c.param.clone()).expect("a dimension with a parameter");
    let LinkSource::Text { exprs, .. } = &sk.links.iter().find(|l| matches!(l.source, LinkSource::Text { .. })).unwrap().source else {
        panic!("not a text")
    };
    (dimension, exprs.height.clone())
}

#[test]
fn a_cloned_components_text_follows_the_cloned_dimension_it_names_not_the_original() {
    let mut s = Session::default();
    let part = run(&mut s, "component.create", json!({"name": "Part"}))["component"].as_u64().unwrap();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [60, 20]}));
    run(&mut s, "sketch.dimension", json!({"entities": ["l1"], "value": 60}));
    let original = s.doc.find_feature("S").unwrap().id;
    let own = s.doc.sketch(original).unwrap().constraints.iter().find_map(|c| c.param.clone()).expect("a dimension with a parameter");
    // The text's height is a quarter of the sketch's own dimension.
    run(&mut s, "sketch.text", json!({"text": "LIVE", "at": [5, 5], "height": format!("{own} / 4")}));
    run(&mut s, "sketch.finish", json!({}));
    no_trouble(&s);
    assert!((outline_height(&s, original) - 15.0).abs() < 1e-6);

    let copy = run(&mut s, "component.paste_new", json!({"component": part, "translate": [0, 50, 0]}))["component"].as_u64().unwrap();
    let clone = s.doc.features.iter().find(|f| f.component == copy && matches!(f.kind, solidcraft_doc::FeatureKind::Sketch { .. })).unwrap().id;
    let (theirs, expression) = dimension_and_expression(&s, clone);
    assert_ne!(theirs, own, "the copy's dimension is its own");
    assert_eq!(expression, Some(format!("{theirs} / 4")), "and so is the expression that uses it");
    no_trouble(&s);
    assert!((outline_height(&s, clone) - 15.0).abs() < 1e-6);
    run(&mut s, "parameters.change", json!({"name": theirs, "expression": "40"}));
    assert!((outline_height(&s, clone) - 10.0).abs() < 1e-6, "the copy follows its own dimension");
    assert!((outline_height(&s, original) - 15.0).abs() < 1e-6, "and the original stays");
    no_trouble(&s);
}

#[test]
fn bare_numbers_in_a_text_expression_are_in_the_designs_units() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
    s.doc_mut().units = "in".into();
    let r = run(&mut s, "sketch.text", json!({"text": "A", "at": [0, 0], "height": "1"}));
    let (_, height, _, exprs) = stored(&s, r["link"].as_str().unwrap());
    // Numbers stay millimetres for programmatic callers; an expression is in the design's units.
    assert!((height - 25.4).abs() < 1e-9 && exprs.is_empty(), "{height}");
    let r = run(&mut s, "sketch.text", json!({"text": "A", "at": [0, 0], "height": 2}));
    assert!((stored(&s, r["link"].as_str().unwrap()).1 - 2.0).abs() < 1e-12);
}

/// A 100 x 30 x 10 mm block with a text cut 1 mm into its face, its words given by `words`
/// (`{"text": …}` or `{"text_expression": …}`). The text's sketch is `S`; the reply is
/// `sketch.text`'s.
fn block_with_text(s: &mut Session, words: Value) -> Value {
    run(s, "sketch.create", json!({"plane": "XY"}));
    run(s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [100, 30]}));
    run(s, "sketch.finish", json!({}));
    run(s, "solid.extrude", json!({"distance": -10}));
    run(s, "sketch.create", json!({"plane": "XY", "name": "S"}));
    let mut p = json!({"at": [5, 10], "height": 8});
    for (k, v) in words.as_object().unwrap() {
        p[k] = v.clone();
    }
    let r = run(s, "sketch.text", p);
    run(s, "sketch.finish", json!({}));
    run(s, "solid.extrude", json!({"distance": -1, "operation": "cut"}));
    r
}

fn volume(s: &mut Session) -> f64 {
    run(s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap()
}

/// The words of the text `link` of sketch `S`, as evaluated.
fn words(s: &Session, link: &str) -> String {
    let id = s.doc.find_feature("S").unwrap().id;
    let st = s.model.state();
    let LinkSource::Text { text, .. } = &st.sketch(id).unwrap().sketch.link(link).unwrap().source else { panic!("not a text") };
    text.clone()
}

/// The expression of the words of the text `link` of sketch `S` (null: typed words).
fn word_expr(s: &Session, link: &str) -> Value {
    serde_json::to_value(stored(s, link).3).unwrap()["text"].clone()
}

/// A text's words can be an expression of text parameters (`Label + '-L'`): a parameter edit,
/// or a configuration row that sets the parameter, relabels the text, and a cut of the text
/// follows in the same rebuild, as if its words had been typed.
#[test]
fn a_texts_words_follow_its_text_parameters_and_so_does_its_cut() {
    let typed = |words: &str| {
        let mut s = Session::default();
        block_with_text(&mut s, json!({"text": words}));
        no_trouble(&s);
        volume(&mut s)
    };
    let same = |a: f64, b: f64, what: &str| assert!((a - b).abs() < 1e-6, "{what}: {a} against {b} typed");
    let mut s = Session::default();
    let r = run(&mut s, "parameters.add", json!({"name": "Label", "expression": "IT", "unit": "text"}));
    assert_eq!((r["unit"].as_str(), r["text"].as_str()), (Some("text"), Some("IT")), "{r}");
    let r = block_with_text(&mut s, json!({"text_expression": "Label + '-L'"}));
    let link = r["link"].as_str().unwrap().to_string();
    assert_eq!(r["exprs"], json!({"text": "Label + '-L'"}), "{r}");
    no_trouble(&s);
    assert_eq!(words(&s, &link), "IT-L");
    same(volume(&mut s), typed("IT-L"), "IT-L");

    // A parameter edit relabels; the cut follows in the same rebuild, and after a save.
    run(&mut s, "parameters.change", json!({"name": "Label", "expression": "WWM"}));
    no_trouble(&s);
    assert_eq!(words(&s, &link), "WWM-L");
    let wide = typed("WWM-L");
    same(volume(&mut s), wide, "WWM-L");
    let mut again = reopened(&s);
    no_trouble(&again);
    same(volume(&mut again), wide, "WWM-L reopened");

    // A configuration row sets it like any parameter.
    run(&mut s, "config.configure", json!({"params": ["Label"]}));
    run(&mut s, "config.row", json!({"name": "Short", "values": {"Label": "I"}}));
    run(&mut s, "config.row", json!({"name": "Wide", "values": {"Label": "WWM"}}));
    run(&mut s, "config.activate", json!({"row": "Short"}));
    no_trouble(&s);
    assert_eq!(words(&s, &link), "I-L");
    same(volume(&mut s), typed("I-L"), "row Short");
    run(&mut s, "config.activate", json!({"row": "Wide"}));
    same(volume(&mut s), wide, "row Wide");

    // Listed with its text; a rename follows into the expression; deleting it is refused while
    // the text uses it.
    let list = run(&mut s, "parameters.list", json!({}));
    let row = list["parameters"].as_array().unwrap().iter().find(|r| r["name"] == "Label").cloned().unwrap();
    assert_eq!((row["unit"].as_str(), row["text"].as_str()), (Some("text"), Some("WWM")), "{row}");
    run(&mut s, "parameters.rename", json!({"name": "Label", "new_name": "Tag"}));
    assert_eq!(word_expr(&s, &link), json!("Tag + '-L'"));
    let users = run(&mut s, "parameters.users", json!({"name": "Tag"}));
    assert!(users["drives"].to_string().contains("\"S\""), "{users}");
    let e = s.execute("parameters.delete", &json!({"name": "Tag"})).unwrap_err().to_string();
    assert!(e.contains("Text"), "{e}");
    no_trouble(&s);
    same(volume(&mut s), wide, "after the rename");

    // Words typed as a text replace the expression.
    run(&mut s, "sketch.edit", json!({"sketch": "S"}));
    run(&mut s, "sketch.edit_text", json!({"link": link, "text": "OK"}));
    run(&mut s, "sketch.finish", json!({}));
    assert_eq!((words(&s, &link).as_str(), word_expr(&s, &link)), ("OK", Value::Null));

    // An expression naming an unknown parameter, or a text parameter in number(), is refused.
    run(&mut s, "sketch.edit", json!({"sketch": "S"}));
    for (bad, says) in [("Nope + 'x'", "Nope"), ("number(Tag)", "Tag"), ("Tag +", "")] {
        let e = s.execute("sketch.edit_text", &json!({"link": link, "text_expression": bad})).unwrap_err().to_string();
        assert!(e.contains(says), "{bad}: {e}");
    }
}

/// #727: a cut that picked every region of a text by index (`profiles: [0, 1]`) follows
/// the text's new words when its text parameter changes, as the whole-sketch cut does: the
/// same number of glyphs (`22` to `25`), a glyph with a counter (`30`) and more glyphs (`177`).
#[test]
fn a_cut_of_a_texts_regions_picked_by_index_follows_its_new_words() {
    let whole = |words: &str| {
        let mut s = Session::default();
        block_with_text(&mut s, json!({"text": words}));
        no_trouble(&s);
        volume(&mut s)
    };
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "Label", "expression": "22", "unit": "text"}));
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [100, 30]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": -10}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
    let link = run(&mut s, "sketch.text", json!({"text_expression": "Label", "at": [5, 10], "height": 8}))["link"].as_str().unwrap().to_string();
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "S", "profiles": [0, 1], "distance": -1, "operation": "cut"}));
    no_trouble(&s);
    let first = volume(&mut s);
    assert!((first - whole("22")).abs() < 1e-6, "22: {first} against {} by the whole sketch", whole("22"));

    for w in ["25", "30", "177"] {
        run(&mut s, "parameters.change", json!({"name": "Label", "expression": w}));
        assert_eq!(words(&s, &link), w);
        no_trouble(&s);
        let (got, want) = (volume(&mut s), whole(w));
        assert!((got - want).abs() < 1e-6, "{w}: {got} against {want} by the whole sketch");
    }

    // Saved as the text, not its curves: it reopens and still follows the words.
    let saved = |s: &Session| serde_json::to_value(&s.doc.find_feature("Extrude2").unwrap().kind).unwrap().to_string();
    assert!(saved(&s).contains(r#""text_regions""#), "{}", saved(&s));
    let mut again = reopened(&s);
    run(&mut again, "parameters.change", json!({"name": "Label", "expression": "22"}));
    no_trouble(&again);
    assert!((volume(&mut again) - first).abs() < 1e-6, "22 after a save");

    // One letter of two stays one letter: a part of a text never grows into the whole text.
    let mut one = Session::default();
    run(&mut one, "parameters.add", json!({"name": "Label", "expression": "22", "unit": "text"}));
    run(&mut one, "sketch.create", json!({"plane": "XY"}));
    run(&mut one, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [100, 30]}));
    run(&mut one, "sketch.finish", json!({}));
    run(&mut one, "solid.extrude", json!({"distance": -10}));
    run(&mut one, "sketch.create", json!({"plane": "XY", "name": "S"}));
    run(&mut one, "sketch.text", json!({"text_expression": "Label", "at": [5, 10], "height": 8}));
    run(&mut one, "sketch.finish", json!({}));
    run(&mut one, "solid.extrude", json!({"sketch": "S", "profiles": [0], "distance": -1, "operation": "cut"}));
    no_trouble(&one);
    assert!(!saved(&one).contains(r#""text_regions""#), "{}", saved(&one));
    let half = 30000.0 - volume(&mut one);
    assert!(half > 0.0 && (half - (30000.0 - first) / 2.0).abs() < 1e-6, "one 2 of 22 cuts half of what both do: {half}");
}

/// #9281: a number parameter in a text's words shows its value in the design's units
/// (`number()` without the unit, with chosen decimals); a parameter edit rewrites the words
/// and the cut follows in the same rebuild, also after a save; a rename reaches the words and
/// deleting the parameter is refused while the text shows it.
#[test]
fn a_texts_words_show_a_number_parameter_and_follow_it() {
    let typed = |words: &str| {
        let mut s = Session::default();
        block_with_text(&mut s, json!({"text": words}));
        no_trouble(&s);
        volume(&mut s)
    };
    let same = |a: f64, b: f64, what: &str| assert!((a - b).abs() < 1e-6, "{what}: {a} against {b} typed");
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "Width", "expression": "85 mm"}));
    run(&mut s, "parameters.add", json!({"name": "Tilt", "expression": "30 deg"}));
    let r = block_with_text(&mut s, json!({"text_expression": "'W' + number(Width)"}));
    let link = r["link"].as_str().unwrap().to_string();
    no_trouble(&s);
    assert_eq!(words(&s, &link), "W85");
    same(volume(&mut s), typed("W85"), "W85");

    run(&mut s, "parameters.change", json!({"name": "Width", "expression": "90 mm"}));
    no_trouble(&s);
    assert_eq!(words(&s, &link), "W90");
    let wide = typed("W90");
    same(volume(&mut s), wide, "W90");
    let mut again = reopened(&s);
    no_trouble(&again);
    same(volume(&mut again), wide, "W90 reopened");

    run(&mut s, "sketch.edit", json!({"sketch": "S"}));
    run(&mut s, "sketch.edit_text", json!({"link": link, "text_expression": "number(Width, 1) + ' / ' + Tilt"}));
    run(&mut s, "sketch.finish", json!({}));
    no_trouble(&s);
    assert_eq!(words(&s, &link), "90.0 / 30°");

    run(&mut s, "parameters.rename", json!({"name": "Width", "new_name": "Span"}));
    assert_eq!(word_expr(&s, &link), json!("number(Span, 1) + ' / ' + Tilt"));
    let e = s.execute("parameters.delete", &json!({"name": "Span"})).unwrap_err().to_string();
    assert!(e.contains("Text"), "{e}");
    no_trouble(&s);
    assert_eq!(words(&s, &link), "90.0 / 30°");
}

#[test]
fn a_parameter_change_is_saved_in_the_texts_words_place_and_outlines() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "p", "expression": "12.3"}));
    run(&mut s, "parameters.add", json!({"name": "hh", "expression": "5"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
    let r = run(&mut s, "sketch.text", json!({"text_expression": "number(p, 1)", "at": ["p", "-hh / 2"], "height": "hh", "align": "center"}));
    let link = r["link"].as_str().unwrap().to_string();
    run(&mut s, "sketch.finish", json!({}));
    // The glyph points as the saved document has them.
    let saved_points = |s: &Session| -> Vec<Vec2> {
        let id = s.doc.find_feature("S").unwrap().id;
        s.doc.sketch(id).unwrap().points.iter().filter(|q| q.link.is_some()).map(|q| q.pos).collect()
    };
    let before = saved_points(&s);
    run(&mut s, "parameters.change", json!({"name": "p", "expression": "45.6"}));
    no_trouble(&s);
    let id = s.doc.find_feature("S").unwrap().id;
    let LinkSource::Text { text, at, .. } = &s.doc.sketch(id).unwrap().link(&link).unwrap().source else { panic!("not a text") };
    assert_eq!(text, "45.6");
    assert!((at.x - 45.6).abs() < 1e-9 && (at.y + 2.5).abs() < 1e-9, "{at:?}");
    let after = saved_points(&s);
    assert_ne!(before, after, "the saved glyph points moved");
    // What is saved is what evaluating it gives.
    let live: Vec<Vec2> = s.model.state().sketch(id).unwrap().sketch.points.iter().filter(|q| q.link.is_some()).map(|q| q.pos).collect();
    assert_eq!(after.len(), live.len());
    for (a, b) in after.iter().zip(&live) {
        assert!((a.x - b.x).abs() < 1e-9 && (a.y - b.y).abs() < 1e-9, "{a:?} {b:?}");
    }
    let json = s.doc.to_json();
    assert!(json.contains("\"text\": \"45.6\""), "the saved file holds the new words");
}

/// The saved text of sketch `S`, and the ends of its curve `id` as the saved sketch has them.
fn saved_text_and_line(s: &Session, link: &str, id: &str) -> (String, Vec2, Vec2) {
    let feature = s.doc.find_feature("S").unwrap().id;
    let sk = s.doc.sketch(feature).unwrap();
    let LinkSource::Text { text, .. } = &sk.link(link).unwrap().source else { panic!("not a text") };
    let curve = sk.curves.iter().find(|c| c.id == id).unwrap();
    let solidcraft_sketch::CurveKind::Line { a, b } = curve.kind else { panic!("not a line") };
    (text.clone(), sk.points[a].pos, sk.points[b].pos)
}

#[test]
fn a_text_with_more_glyphs_leaves_the_lines_made_after_it_on_their_own_points() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "p", "expression": "1"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
    let r = run(&mut s, "sketch.text", json!({"text_expression": "number(p, 0)", "at": [0, 0], "height": 5}));
    let link = r["link"].as_str().unwrap().to_string();
    run(&mut s, "sketch.line", json!({"points": [[100, 0], [110, 5]], "ids": ["ext"]}));
    run(&mut s, "sketch.finish", json!({}));
    let (_, a, b) = saved_text_and_line(&s, &link, "ext");
    assert!((a.x - 100.0).abs() < 1e-9 && (b.x - 110.0).abs() < 1e-9, "{a:?} {b:?}");
    run(&mut s, "parameters.change", json!({"name": "p", "expression": "4567"}));
    no_trouble(&s);
    let (text, a, b) = saved_text_and_line(&s, &link, "ext");
    assert_eq!(text, "4567");
    assert!((a.x - 100.0).abs() < 1e-9 && a.y.abs() < 1e-9 && (b.x - 110.0).abs() < 1e-9 && (b.y - 5.0).abs() < 1e-9, "{a:?} {b:?}");
}

#[test]
fn an_offset_that_does_not_follow_the_text_does_not_keep_the_saved_text_stale() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "p", "expression": "1"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
    let r = run(&mut s, "sketch.text", json!({"text_expression": "number(p, 0)", "at": [0, 0], "height": 5}));
    let link = r["link"].as_str().unwrap().to_string();
    run(&mut s, "sketch.line", json!({"points": [[100, 0], [110, 0]], "ids": ["ext"]}));
    run(&mut s, "sketch.offset", json!({"curves": ["ext"], "distance": 3.0, "side": [105, 10]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "parameters.change", json!({"name": "p", "expression": "4567"}));
    no_trouble(&s);
    let (text, a, b) = saved_text_and_line(&s, &link, "ext");
    assert_eq!(text, "4567");
    assert!((a.x - 100.0).abs() < 1e-9 && (b.x - 110.0).abs() < 1e-9, "{a:?} {b:?}");
}
