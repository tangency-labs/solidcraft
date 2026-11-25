use serde_json::json;
use solidcore_record::Vec2;
use solidcraft_doc::drawing::note::Note;
use solidcraft_doc::ids;

use super::*;
use crate::drawing_scene::tests::{in_the_model, plate as sheet_plate, with_the_model};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"))
}

fn refused(s: &mut Session, p: Value) -> String {
    s.execute(ADD, &p).err().map(|e| e.to_string()).unwrap_or_else(|| panic!("accepted: {p}"))
}

fn flush_nut(thread: &str, x: f64) -> Value {
    json!({"kind": "flush_nut", "thread": thread, "position": [x, 30, 2]})
}

/// The flanged plate of the flat pattern view's tests with two M5 flush nuts (a feature each) and a
/// flat pattern view of it, centred at (200, 150): the session, the nuts' features and the view.
fn nuts() -> (Session, [u64; 2], u64) {
    let mut s = sheet_plate();
    let first = in_the_model(&mut s, "sheet.hardware", flush_nut("M5", 20.0))["feature"].as_u64().unwrap();
    let second = in_the_model(&mut s, "sheet.hardware", flush_nut("M5", 80.0))["feature"].as_u64().unwrap();
    let p = json!({"sheet": "Sheet 1", "body": "Plate", "scale": "1:1", "position_mm": [200, 150]});
    let view = ids::decode(run(&mut s, "drawing.view.flat", p)["view"].as_str().unwrap()).unwrap();
    (s, [first, second], view)
}

fn add(s: &mut Session, view: u64, feature: u64) -> Value {
    run(s, ADD, json!({"view": ids::encode(view), "feature": feature, "at": [60, 40]}))
}

fn note_id(reply: &Value) -> u64 {
    ids::decode(reply["note"].as_str().unwrap()).unwrap()
}

fn inspect(s: &mut Session, note: u64) -> Value {
    run(s, "drawing.note.inspect", json!({"note": ids::encode(note)}))
}

fn flat_hardware(s: &mut Session) -> Vec<Value> {
    with_the_model(s, |s| run(s, "sheet.flat_pattern", json!({})))["hardware"].as_array().cloned().unwrap_or_default()
}

/// A callout on a flat pattern view reads "2× FLUSH NUT M5" for two such nuts, is a note that
/// saves the hardware reference, and ends on its hole's centre in the flat, as `sheet.flat_pattern` reports it.
#[test]
fn a_flat_view_callout_reads_the_record_and_ends_on_its_hole() {
    let (mut s, [first, second], view) = nuts();
    let reply = add(&mut s, view, first);
    assert_eq!(reply["text"], "2× FLUSH NUT M5", "{reply}");
    assert_eq!(reply["status"], "ok", "{reply}");
    assert_eq!(reply["record"]["hardware"], json!({"feature": first, "copy": 0}), "{reply}");
    let other = note_id(&add(&mut s, view, second));
    let d = s.drawing("test").unwrap().clone();
    let listed = flat_hardware(&mut s);
    let at = |feature: u64| {
        let found = listed.iter().find(|h| h["feature"].as_u64() == Some(feature)).unwrap();
        Vec2::new(found["position"][0].as_f64().unwrap(), found["position"][1].as_f64().unwrap())
    };
    for (note, feature) in [(note_id(&reply), first), (other, second)] {
        let tip = crate::drawing_hardware_callout::flat_tip(&s, &d, d.notes.by_id(note).unwrap(), "test").unwrap();
        assert!((tip - at(feature)).len() < 1e-9, "{tip:?} {:?}", at(feature));
    }
    assert!((at(first) - at(second)).len() > 1.0, "two holes");
    let sheet = d.view(view).unwrap().sheet;
    let built = crate::drawing_scene::build(&s, &d, sheet).unwrap();
    assert!(built.notes.iter().all(|n| n.drawn && n.status == solidcraft_doc::drawing::dimension::Status::Ok), "{:?}", built.notes);
    assert_eq!(built.notes.len(), 2);
}

/// The text is the record's now: the nut's thread changed in the model is the callout's text on the
/// next read, and a nut the model no longer has makes the callout lost, keeping its last text, with no leader.
#[test]
fn a_callout_follows_the_record_and_a_deleted_hardware_makes_it_lost() {
    let (mut s, [first, _], view) = nuts();
    let note = note_id(&add(&mut s, view, first));
    in_the_model(
        &mut s,
        "timeline.redefine",
        json!({"feature": first, "command": "sheet.hardware", "params": {"kind": "flush_nut", "thread": "M4", "position": [20, 30, 2]}}),
    );
    // The M4 nut is alone of its kind now.
    assert_eq!(inspect(&mut s, note)["text"], "FLUSH NUT M4");
    in_the_model(&mut s, "timeline.delete", json!({"feature": first}));
    let lost = inspect(&mut s, note);
    assert_eq!(lost["status"], "lost", "{lost}");
    assert_eq!(lost["text"], "2× FLUSH NUT M5", "it keeps the text it had: {lost}");
    let d = s.drawing("test").unwrap().clone();
    let built = crate::drawing_scene::build(&s, &d, d.view(view).unwrap().sheet).unwrap();
    assert!(built.notes.iter().all(|n| matches!(n.status, solidcraft_doc::drawing::dimension::Status::Lost(..))), "{:?}", built.notes);
    assert!(crate::drawing_hardware_callout::flat_tip(&s, &d, d.notes.by_id(note).unwrap(), "test").is_none(), "no hole, no leader");
}

/// On a view of the model the text reads the record the same way (the leader is the note's anchor).
#[test]
fn a_callout_on_a_view_of_the_model_reads_the_record() {
    let (mut s, [first, _], _) = nuts();
    let sheet = ids::encode(s.drawing("test").unwrap().sheets[0].id);
    let p = json!({"sheet": sheet, "view": "front", "scale": "1:1", "position_mm": [100, 100]});
    let front = ids::decode(run(&mut s, "drawing.view.base", p)["view"].as_str().unwrap()).unwrap();
    let e = refused(&mut s, json!({"view": ids::encode(front), "feature": first, "at": [10, 10]}));
    assert!(e.contains("`corner`"), "a view of the model needs the hole's centre: {e}");
    let mut d = (**s.drawing("test").unwrap()).clone();
    let id = d.allocate_id(&mut s.ids).unwrap();
    let note = Note {
        id,
        anchor: solidcraft_doc::drawing::note::Anchor::View(front),
        at: [10.0, 10.0],
        content: solidcraft_geom::cell::Content(vec![solidcraft_geom::cell::Part::Text("old".into())]),
        cap_mm: None,
        align: None,
        branches: Vec::new(),
        hardware: Some(solidcraft_doc::drawing::hardware_callout::HardwareRef { feature: first, copy: 0 }),
    };
    d.notes.add([Arc::new(note)]);
    d.validate().unwrap();
    s.doc_mut().put_drawing(Arc::new(d));
    assert_eq!(inspect(&mut s, id)["text"], "2× FLUSH NUT M5");
}

/// Refused: a feature with no hardware, one that isn't there, a flat view of another body, a
/// `corner` on a flat view, a copy no feature has, and a place off the paper; nothing is added.
#[test]
fn a_hardware_callout_refuses_what_it_cannot_read() {
    let (mut s, [first, _], view) = nuts();
    let plain = in_the_model(&mut s, "solid.hole", json!({"position": [50, 10, 2], "diameter": 3}))["feature"].as_u64().unwrap();
    let v = ids::encode(view);
    assert!(refused(&mut s, json!({"view": v, "feature": plain, "at": [0, 0]})).contains("no press-in hardware"));
    assert!(refused(&mut s, json!({"view": v, "feature": 99999, "at": [0, 0]})).contains("no press-in hardware"));
    assert!(refused(&mut s, json!({"view": v, "feature": first, "copy": 1, "at": [0, 0]})).contains("no hardware hole 1"));
    assert!(refused(&mut s, json!({"view": v, "feature": first, "copy": 10000, "at": [0, 0]})).contains("copy"));
    assert!(refused(&mut s, json!({"view": v, "feature": first, "at": [0, 0], "corner": {"point": [0, 0, 0]}})).contains("no `corner`"));
    assert!(refused(&mut s, json!({"view": v, "feature": first, "at": [1e300, 0]})).contains("on paper"));
    assert!(refused(&mut s, json!({"view": ids::encode(1), "feature": first, "at": [0, 0]})).contains("no view"));
    assert!(refused(&mut s, json!({"view": v, "feature": 0, "at": [0, 0]})).contains("feature"));
    assert!(refused(&mut s, json!({"view": v, "feature": first, "at": [0, 0], "extra": 1})).contains("extra"));
    assert!(s.drawing("test").unwrap().notes.is_empty(), "nothing was added");
}

/// Adding is one undo step, and the note is a note: edited and deleted as one.
#[test]
fn adding_a_hardware_callout_is_one_undo_step() {
    let (mut s, [first, _], view) = nuts();
    let note = note_id(&add(&mut s, view, first));
    run(&mut s, "edit.undo", json!({}));
    assert!(s.drawing("test").unwrap().notes.by_id(note).is_none());
}
