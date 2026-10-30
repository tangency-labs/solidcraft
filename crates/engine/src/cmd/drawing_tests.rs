use super::*;
use crate::cmd::{Part, command_specs, find_command};
use crate::tests::Folder;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"))
}

fn refused(s: &mut Session, id: &str, p: Value) -> String {
    s.execute(id, &p).err().map(|e| e.to_string()).unwrap_or_else(|| panic!("{id} accepted {p}"))
}

/// A design with a box.
fn design() -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 10, "body_name": "Plate"}));
    s
}

/// The drawing open, as the design holds it.
fn open_one(s: &Session) -> Arc<Drawing> {
    s.drawing("test").unwrap().clone()
}

/// The params naming drawing `id`.
fn the(id: u64) -> Value {
    json!({"drawing": ids::encode(id)})
}

fn names(s: &Session) -> Vec<&str> {
    s.doc.drawings.iter().map(|d| d.name.as_str()).collect()
}

/// A drawing of the design: one A3 sheet with the ISO border, the design its one source, kept in
/// the design and opened. Making it is one step of the model, and evaluates nothing.
#[test]
fn a_drawing_of_the_design_is_kept_in_it_and_opened() {
    let mut s = design();
    let (undo, evaluations, features) = (s.undo.len(), s.evaluations, s.doc.features.clone());
    let made = run(&mut s, "drawing.create", json!({"name": "Plate drawing", "units": "mm"}));
    assert_eq!((made["name"].as_str(), made["language"].as_str()), (Some("Plate drawing"), Some("en")));
    assert_eq!(made["units"], json!({"fixed": {"unit": "mm"}}));
    let sheet = &made["sheets"][0];
    assert_eq!(
        (sheet["size_mm"].clone(), sheet["orientation"].clone(), sheet["dpi"].clone()),
        (json!([420.0, 297.0]), json!("landscape"), json!(300.0))
    );
    assert_eq!((made["sheets"].as_array().map(Vec::len), made["sources"].as_array().map(Vec::len)), (Some(1), Some(1)));
    let d = open_one(&s);
    assert_eq!(made["id"], json!(ids::encode(d.id)));
    assert!(d.sheets[0].border.is_some(), "the ISO border");
    assert_eq!((s.doc.drawings.len(), s.open_drawing), (1, Some(d.id)));
    assert_eq!((s.undo.len(), s.undo.last().map(|u| u.part)), (undo + 1, Some(None)), "a step of the model");
    assert_eq!((s.evaluations, &s.doc.features), (evaluations, &features), "nothing evaluated, the model as it was");
    assert!(s.is_dirty());
    assert_eq!(run(&mut s, "drawing.inspect", json!({})), made);
    let listed = run(&mut s, "drawing.list", json!({}));
    assert_eq!(listed, json!({"drawings": [{"id": made["id"], "name": "Plate drawing", "sheets": 1, "open": true}]}));
    // Its name is the design's unless given.
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "drawing.create", json!({}));
    assert_eq!(open_one(&s).name, s.doc.name);
}

/// A drawing's ids come from the session's id source: two sessions seeded alike make a drawing
/// with the same ids (where a crash replay starts), another seed other ids. A file change keeps
/// the source, so the same holds for a drawing made after `file.new`.
#[test]
fn drawing_ids_come_from_the_sessions_seed() {
    let ids = |seed: u64, new_file: bool| {
        let mut s = design();
        s.seed_ids(seed);
        if new_file {
            run(&mut s, "file.new", json!({}));
            run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 10, "body_name": "Plate"}));
        }
        run(&mut s, "drawing.create", json!({}));
        let d = open_one(&s);
        (d.id, d.sheets[0].id, d.sources[0].id)
    };
    assert_eq!(ids(7, false), ids(7, false));
    assert_ne!(ids(7, false), ids(8, false));
    assert_eq!(ids(7, true), ids(7, true), "the source kept across the file change");
    assert_ne!(ids(7, true), ids(8, true));
}

/// What making a drawing refuses, the design unchanged: an empty design, units, a language and a
/// name the record can't take, and a design that holds the most drawings already.
#[test]
fn a_drawing_is_made_only_of_a_design_with_something_in_it() {
    assert!(refused(&mut Session::default(), "drawing.create", json!({})).contains("nothing to draw"));
    let mut s = design();
    let before = s.doc.clone();
    for (p, says) in [
        (json!({"units": "parsec"}), "length unit"),
        (json!({"units": 3}), "follow_model"),
        (json!({"language": "e"}), "BCP 47"),
        (json!({"name": " "}), "name"),
        (json!({"name": 7}), "must be text"),
    ] {
        let e = refused(&mut s, "drawing.create", p.clone());
        assert!(e.contains(says), "{p}: {e}");
        assert!(Arc::ptr_eq(&s.doc, &before) && s.open_drawing.is_none() && s.undo.len() == 1, "{p}: nothing changed");
    }
    run(&mut s, "drawing.create", json!({}));
    run(&mut s, "drawing.close", json!({}));
    let one = s.doc.drawings[0].clone();
    s.doc_mut().drawings = (1..=drawing::MAX_DRAWINGS as u64).map(|id| Arc::new(Drawing { id, ..(*one).clone() })).collect();
    let full = s.doc.clone();
    assert!(refused(&mut s, "drawing.create", json!({})).contains("at most 1000 drawings"));
    assert!(Arc::ptr_eq(&s.doc, &full));
}

/// While a drawing is open the model's commands wait, and the drawing's run only while one is:
/// running a command, running it inside another and listing it all say so.
#[test]
fn model_and_drawing_commands_each_run_on_their_own_part() {
    let mut s = design();
    for id in ["drawing.inspect", "drawing.set", "drawing.properties", "drawing.properties.set", "drawing.properties.remove"] {
        assert!(refused(&mut s, id, json!({"name": "X", "key": "author", "value": "X"})).contains("open a drawing first"), "{id}");
    }
    run(&mut s, "drawing.create", json!({}));
    let doc = s.doc.clone();
    let boxed = json!({"length": 5, "width": 5, "height": 5});
    let e = refused(&mut s, "solid.box", boxed.clone());
    assert!(e.contains("a drawing is open: close it to change the model (drawing.close)"), "{e}");
    assert!(find_command("solid.box").unwrap().run_nested(&mut s, &boxed).is_err(), "nor inside another command");
    assert!(Arc::ptr_eq(&s.doc, &doc));
    let enabled = |s: &mut Session, id: &str| {
        let listed = run(s, "engine.commands", json!({}));
        listed.as_array().unwrap().iter().find(|c| c["id"] == id).map(|c| c["enabled"] == true).unwrap()
    };
    assert!(!enabled(&mut s, "solid.box") && enabled(&mut s, "drawing.set"));
    for id in ["edit.undo", "file.save", "file.new", "doc.open", "select.clear", "edit.checkpoint", "drawing.list", "drawing.close"] {
        assert!(find_command(id).unwrap().allowed(&s).is_ok(), "{id} runs with a drawing open");
    }
    run(&mut s, "drawing.close", json!({}));
    assert!(enabled(&mut s, "solid.box") && !enabled(&mut s, "drawing.set"));
    run(&mut s, "solid.box", boxed);
    // A new design or another opened has no drawing open.
    run(&mut s, "drawing.create", json!({}));
    run(&mut s, "file.new", json!({}));
    assert!(s.open_drawing.is_none() && s.doc.drawings.is_empty());
}

/// Each change of a drawing is one undo step of that drawing: undo and redo bring back the same
/// record, and a change to the same values is no step. No model is evaluated, and the design
/// changes, so whatever follows it sees each change.
#[test]
fn drawing_edits_are_undo_steps_of_the_drawing_that_evaluate_nothing() {
    let mut s = design();
    run(&mut s, "drawing.create", json!({}));
    let (first, evaluations, features, steps) = (open_one(&s), s.evaluations, s.doc.features.clone(), s.undo.len());
    let (doc, revision) = (s.doc.clone(), s.revision);
    run(&mut s, "drawing.set", json!({"name": "Renamed", "language": "de", "units": "in"}));
    let renamed = open_one(&s);
    assert_eq!((renamed.name.as_str(), renamed.language.as_str()), ("Renamed", "de"));
    assert_eq!(renamed.units, Units::Fixed { unit: "in".into() });
    assert!(!Arc::ptr_eq(&s.doc, &doc) && s.revision > revision, "the design changed");
    assert_eq!((s.undo.len(), s.undo.last().map(|u| u.part)), (steps + 1, Some(Some(first.id))), "a step of the drawing");
    run(&mut s, "drawing.set", json!({"name": "Renamed"}));
    assert_eq!(s.undo.len(), steps + 1, "the same value again is no step");
    for _ in 0..100 {
        assert_eq!(run(&mut s, "edit.undo", json!({}))["undone"], "Drawing Settings");
        assert!(Arc::ptr_eq(&open_one(&s), &first));
        run(&mut s, "edit.redo", json!({}));
        assert!(Arc::ptr_eq(&open_one(&s), &renamed));
    }
    assert_eq!((s.evaluations, &s.doc.features), (evaluations, &features), "no model evaluated or changed");
}

/// A drawing's projection is first or third angle, third when nothing chooses: shown by
/// `drawing.inspect`, changed by `drawing.set` as one undo step, saved with the design, and
/// anything else refused.
#[test]
fn a_drawings_projection_is_first_or_third_angle() {
    let folder = Folder::new("drawing-projection");
    let mut s = design();
    let made = run(&mut s, "drawing.create", json!({}));
    assert_eq!(made["projection"], "third");
    let steps = s.undo.len();
    assert_eq!(run(&mut s, "drawing.set", json!({"projection": "first"}))["projection"], "first");
    assert_eq!(s.undo.len(), steps + 1);
    run(&mut s, "file.save", json!({"path": folder.path("plate.solidcraft")}));
    let mut opened = Session::default();
    run(&mut opened, "doc.open", json!({"path": folder.path("plate.solidcraft")}));
    assert_eq!(opened.doc.drawings[0].projection, Projection::First);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(run(&mut s, "drawing.inspect", json!({}))["projection"], "third");
    for bad in [json!("second"), json!("First"), json!(1), json!(null)] {
        assert!(refused(&mut s, "drawing.set", json!({"projection": bad})).contains("first or third"), "{bad}");
    }
}

/// A drawing's font is a family named by `drawing.create` or `drawing.set` (one undo step), saved
/// with the design; the built-in font when none is named. A family this computer lacks is named in
/// `font_note`, and a font file or anything but text is refused.
#[test]
fn a_drawings_font_is_a_family_by_name() {
    let folder = Folder::new("drawing-font");
    let mut s = design();
    let made = run(&mut s, "drawing.create", json!({"font": " No Such Family Q7 "}));
    assert_eq!(
        (&made["font"], &made["font_note"]),
        (&json!("No Such Family Q7"), &json!("No Such Family Q7 isn't on this computer: set in Liberation Sans"))
    );
    let steps = s.undo.len();
    let set = run(&mut s, "drawing.set", json!({"font": "Liberation Sans"}));
    assert_eq!((&set["font"], &set["font_note"], s.undo.len()), (&json!("Liberation Sans"), &Value::Null, steps + 1));
    run(&mut s, "file.save", json!({"path": folder.path("plate.solidcraft")}));
    let mut opened = Session::default();
    run(&mut opened, "doc.open", json!({"path": folder.path("plate.solidcraft")}));
    assert_eq!(opened.doc.drawings[0].font, "Liberation Sans");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(run(&mut s, "drawing.inspect", json!({}))["font"], "No Such Family Q7");
    assert_eq!(run(&mut s, "drawing.set", json!({"font": ""}))["font_note"], Value::Null);
    for path in ["C:/Fonts/arial.ttf", "/usr/share/fonts/a", "Futura.otf"] {
        let e = refused(&mut s, "drawing.set", json!({"font": path}));
        assert!(e.contains("drawings name fonts by family, so they travel"), "{path}: {e}");
        assert!(refused(&mut s, "drawing.create", json!({"font": path})).contains("by family"), "{path}");
    }
    assert!(refused(&mut s, "drawing.set", json!({"font": 3})).contains("`font` must be text"));
    assert_eq!(open_one(&s).font, "");
}

/// Undo and redo go through the open part's steps only. With a drawing open, its own steps; the
/// model's stay where they are. With none open, the model's steps, which put the design back as
/// it was but keep each drawing it had as it is now.
#[test]
fn undo_and_redo_go_through_one_part_at_a_time() {
    let mut s = design();
    run(&mut s, "drawing.create", json!({"name": "A"}));
    let id = open_one(&s).id;
    run(&mut s, "drawing.set", json!({"name": "B"}));
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5}));
    let two = s.doc.features.len();
    run(&mut s, "drawing.open", the(id));
    run(&mut s, "drawing.set", json!({"name": "C"}));
    // In the drawing: C, B, A; the model's box stays.
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!((open_one(&s).name.as_str(), s.doc.features.len()), ("A", two));
    assert!(refused(&mut s, "edit.undo", json!({})).contains("nothing to undo in this drawing (close it to undo in the model)"));
    // In the model: the box goes and the drawing stays as it is now (A, where the step had B);
    // then the drawing goes.
    run(&mut s, "drawing.close", json!({}));
    let evaluations = s.evaluations;
    run(&mut s, "edit.undo", json!({}));
    assert_eq!((s.doc.features.len(), names(&s)), (two - 1, vec!["A"]));
    assert!(s.evaluations > evaluations, "the model's step evaluates the model");
    run(&mut s, "edit.undo", json!({}));
    assert!(s.doc.drawings.is_empty());
    // Redo: the drawing as it was when it went, then the box, the drawing kept.
    run(&mut s, "edit.redo", json!({}));
    assert_eq!((names(&s), s.open_drawing), (vec!["A"], None));
    run(&mut s, "edit.redo", json!({}));
    assert_eq!((s.doc.features.len(), names(&s)), (two, vec!["A"]));
    assert_eq!(refused(&mut s, "edit.redo", json!({})), "nothing to redo");
    // The drawing's own redo steps were kept all along.
    run(&mut s, "drawing.open", the(id));
    run(&mut s, "edit.redo", json!({}));
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(open_one(&s).name, "C");
    assert!(refused(&mut s, "edit.redo", json!({})).contains("nothing to redo in this drawing"));
}

/// A new step ends what could be redone in its own part only: the others' redo steps still hold.
#[test]
fn a_new_step_ends_what_could_be_redone_in_its_part_only() {
    let mut s = design();
    run(&mut s, "drawing.create", json!({"name": "A"}));
    let id = open_one(&s).id;
    run(&mut s, "drawing.set", json!({"name": "B"}));
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5}));
    run(&mut s, "drawing.open", the(id));
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(open_one(&s).name, "B", "a model step leaves the drawing's redo");
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "drawing.set", json!({"name": "Z"}));
    assert!(refused(&mut s, "edit.redo", json!({})).contains("nothing to redo in this drawing"), "a drawing step ends its own");
    run(&mut s, "drawing.close", json!({}));
    let features = s.doc.features.len();
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "drawing.open", the(id));
    run(&mut s, "drawing.set", json!({"name": "Q"}));
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "edit.redo", json!({}));
    assert_eq!((s.doc.features.len(), names(&s)), (features, vec!["Q"]), "a drawing step leaves the model's redo");
}

/// A design lists its drawings by name, then id, whatever order they were made, renamed or
/// undone in.
#[test]
fn a_designs_drawings_are_listed_by_name_then_id() {
    let mut s = design();
    for name in ["Zeta", "Alpha", "Mid", "Alpha"] {
        run(&mut s, "drawing.create", json!({"name": name}));
    }
    assert_eq!(names(&s), ["Alpha", "Alpha", "Mid", "Zeta"]);
    assert!(s.doc.drawings[0].id < s.doc.drawings[1].id);
    let first = s.doc.drawings[0].id;
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5}));
    run(&mut s, "drawing.open", the(first));
    run(&mut s, "drawing.set", json!({"name": "Zz"}));
    assert_eq!(names(&s), ["Alpha", "Mid", "Zeta", "Zz"]);
    let listed = run(&mut s, "drawing.list", json!({}));
    let open: Vec<bool> = listed["drawings"].as_array().unwrap().iter().map(|d| d["open"] == true).collect();
    assert_eq!(open, [false, false, false, true]);
    // The model's step, made before the rename, keeps the drawing renamed and in its place.
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(names(&s), ["Alpha", "Mid", "Zeta", "Zz"]);
    run(&mut s, "drawing.open", the(first));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(names(&s), ["Alpha", "Alpha", "Mid", "Zeta"]);
}

/// Deleting a drawing is a step of the model: it closes it when it was open, and undo brings it
/// back as it was, closed. What isn't a drawing of the design is refused.
#[test]
fn deleting_a_drawing_is_a_step_of_the_model() {
    let mut s = design();
    run(&mut s, "drawing.create", json!({"name": "A"}));
    let a = open_one(&s);
    run(&mut s, "drawing.create", json!({"name": "B"}));
    let b = open_one(&s);
    let doc = s.doc.clone();
    for (p, says) in [(json!({}), "must be a drawing's id"), (json!({"drawing": 7}), "must be a drawing's id"), (the(1), "has no drawing")] {
        let e = refused(&mut s, "drawing.delete", p.clone());
        assert!(e.contains(says), "{p}: {e}");
        assert!(Arc::ptr_eq(&s.doc, &doc) && s.open_drawing == Some(b.id));
    }
    assert_eq!(run(&mut s, "drawing.delete", the(b.id)), json!({"deleted": ids::encode(b.id)}));
    assert_eq!((names(&s), s.open_drawing), (vec!["A"], None));
    run(&mut s, "drawing.delete", the(a.id));
    assert!(s.doc.drawings.is_empty());
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    assert!(Arc::ptr_eq(&s.doc.drawings[0], &a) && Arc::ptr_eq(&s.doc.drawings[1], &b), "back as they were");
    assert_eq!(s.open_drawing, None);
    assert!(refused(&mut s, "drawing.open", the(1)).contains("has no drawing"));
}

/// A refused change leaves the drawing as it was, with no step, and a refused opening leaves the
/// drawing open as it was.
#[test]
fn a_refused_change_leaves_the_drawing_as_it_was() {
    let mut s = design();
    run(&mut s, "drawing.create", json!({}));
    let (before, steps) = (s.doc.clone(), s.undo.len());
    for p in [json!({"language": "e"}), json!({"name": ""}), json!({"units": "kg"}), json!({"name": "x".repeat(300)})] {
        refused(&mut s, "drawing.set", p.clone());
        assert!(Arc::ptr_eq(&s.doc, &before) && s.undo.len() == steps, "{p}");
    }
    let open = s.open_drawing;
    refused(&mut s, "drawing.open", json!({"drawing": "AAAAAAAAAAAAA"}));
    assert_eq!(s.open_drawing, open);
}

/// A checkpoint brings back the design's drawings, the one open and the undo history. Going back
/// over a change of the model and of a drawing evaluates the model: it is no drawing-only change.
#[test]
fn a_checkpoint_brings_back_the_drawings_and_the_one_open() {
    let mut s = design();
    run(&mut s, "drawing.create", json!({"name": "A"}));
    let a = open_one(&s);
    let (checkpoint, steps, features) = (run(&mut s, "edit.checkpoint", json!({}))["checkpoint"].clone(), s.undo.len(), s.doc.features.clone());
    run(&mut s, "drawing.set", json!({"name": "B"}));
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10}));
    run(&mut s, "drawing.delete", the(a.id));
    run(&mut s, "drawing.create", json!({"name": "C"}));
    let evaluations = s.evaluations;
    run(&mut s, "edit.restore_checkpoint", json!({"checkpoint": checkpoint}));
    assert!(s.doc.drawings.len() == 1 && Arc::ptr_eq(&s.doc.drawings[0], &a));
    assert_eq!((s.open_drawing, s.undo.len(), &s.doc.features), (Some(a.id), steps, &features));
    assert!(s.evaluations > evaluations, "the model changed too, so it is evaluated");
}

/// A design's drawings are saved in its file and come back with it, as a container and as plain
/// JSON, with a drawing open when it is saved; a design opened has none open.
#[test]
fn a_designs_drawings_are_saved_with_it_and_come_back() {
    let folder = Folder::new("drawing-saved");
    let mut s = design();
    run(&mut s, "drawing.create", json!({"name": "A", "language": "en-GB"}));
    run(&mut s, "drawing.properties.set", json!({"values": {"author": "Jane Doe", "com.acme.shop": {"shop_id": 88}}}));
    run(&mut s, "drawing.create", json!({"name": "B", "units": "mm"}));
    let drawings = s.doc.drawings.clone();
    for file in ["plate.solidcraft", "plate.json"] {
        run(&mut s, "file.save", json!({"path": folder.path(file)}));
        assert!(!s.is_dirty(), "{file}");
        let mut opened = Session::default();
        run(&mut opened, "doc.open", json!({"path": folder.path(file)}));
        assert_eq!(opened.doc.drawings, drawings, "{file}");
        assert!(opened.open_drawing.is_none(), "{file}");
    }
}

/// A drawing's properties follow the design properties' rules: set, read and removed, each change
/// one undo step, other tools' keys kept, and what the design's would refuse refused.
#[test]
fn a_drawings_properties_follow_the_design_properties_rules() {
    let mut s = design();
    run(&mut s, "drawing.create", json!({}));
    let steps = s.undo.len();
    let set = run(&mut s, "drawing.properties.set", json!({"values": {"author": "Jane Doe", "com.acme.shop": {"shop_id": 88}}}));
    assert_eq!(set["changed"], json!(["author", "com.acme.shop"]));
    assert_eq!(s.undo.len(), steps + 1);
    let read = run(&mut s, "drawing.properties", json!({}));
    assert_eq!((read["properties"]["author"].clone(), read["metadata"]["com.acme.shop"]["shop_id"].clone()), (json!("Jane Doe"), json!(88)));
    assert_eq!(read["properties"]["revision"], Value::Null);
    run(&mut s, "drawing.properties.set", json!({"key": "author", "value": "Jane Doe"}));
    assert_eq!(s.undo.len(), steps + 1, "the same value again is no change");
    let removed = run(&mut s, "drawing.properties.remove", json!({"keys": ["author", "missing"]}));
    assert_eq!(removed["removed"], json!(["author"]));
    assert_eq!((open_one(&s).properties.text("author"), s.undo.len()), (None, steps + 2));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(open_one(&s).properties.text("author"), Some("Jane Doe"));
    assert!(s.doc.metadata.text("author").is_none(), "the design's own properties untouched");
    let before = open_one(&s);
    for (id, p, says) in [
        ("drawing.properties.set", json!({"key": "bom_type", "value": "purchased"}), "component.metadata.set"),
        ("drawing.properties.set", json!({"key": "author"}), "`value` is required"),
        ("drawing.properties.set", json!({"values": {"author": 7}}), "author"),
        ("drawing.properties.remove", json!({"keys": [1]}), "list of keys"),
    ] {
        let e = refused(&mut s, id, p.clone());
        assert!(e.contains(says), "{p}: {e}");
        assert!(Arc::ptr_eq(&open_one(&s), &before));
    }
}

/// Hostile values for each key of every command that runs with a drawing open, from the registry,
/// and as the whole params, each on a copy of one session with a drawing open (the shared pass
/// runs with none, where the part gate refuses them): an error or a result, never a panic, and the
/// design's drawings and the one open still keeping their rules. As in the shared pass, what
/// writes files is left to its own tests; the recovery commands never read a drawing.
#[test]
fn hostile_drawing_params_never_panic() {
    let mut s = design();
    run(&mut s, "drawing.create", json!({}));
    run(&mut s, "drawing.create", json!({"name": "Second"}));
    assert_eq!(s.scratch().open_drawing, s.open_drawing, "each copy has the drawing open");
    let keys = ["name", "language", "units", "drawing", "key", "value", "values", "keys", "checkpoint", "id"];
    let skipped = |id: &str| matches!(id, "doc.open" | "file.save" | "file.save_as") || id.starts_with("doc.recover");
    let mut internal = Vec::new();
    for spec in command_specs().iter().filter(|spec| spec.part != Part::Model && !skipped(spec.id)) {
        for value in crate::tests::hostile_values() {
            for p in keys.iter().map(|key| json!({ *key: value.clone() })).chain([value.clone()]) {
                let mut copy = s.scratch();
                if let Err(crate::EngineError::Internal(_, msg)) = copy.execute(spec.id, &p) {
                    internal.push(format!("{} {p}: {msg}", spec.id));
                }
                drawing::check_in_design(&copy.doc.drawings).unwrap();
                copy.doc.drawings.iter().for_each(|d| d.validate().unwrap());
                assert!(copy.open_drawing.is_none_or(|id| copy.doc.drawing(id).is_some()), "{} {p}", spec.id);
            }
        }
    }
    assert!(internal.is_empty(), "commands panicked with a drawing open:\n{}", internal.join("\n"));
}

/// Making, opening, closing, changing and deleting drawings, model edits, undo, redo and
/// checkpoints in any order: never a panic, and the design's drawings and the one open always
/// keep their rules.
#[test]
fn drawing_steps_in_any_order_keep_the_rules() {
    let mut state = 3890_u64;
    let mut next = move |n: u64| {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (state >> 33) % n
    };
    for _ in 0..12 {
        let mut s = design();
        let mut checkpoints = Vec::new();
        for _ in 0..50 {
            let pick = next(8) as usize;
            let named = s.doc.drawings.get(pick % s.doc.drawings.len().max(1)).map_or(json!({"drawing": "AAAAAAAAAAAAA"}), |d| the(d.id));
            let (id, p) = match next(11) {
                0 => ("drawing.create", json!({"name": format!("D{}", next(4))})),
                1 => ("drawing.open", named),
                2 => ("drawing.close", json!({})),
                3 => ("drawing.set", json!({"name": format!("D{}", next(4))})),
                4 => ("drawing.delete", named),
                5 | 6 => ("edit.undo", json!({})),
                7 => ("edit.redo", json!({})),
                8 => ("solid.box", json!({"length": 1 + next(9), "width": 3, "height": 3})),
                9 => ("drawing.properties.set", json!({"key": "author", "value": format!("A{}", next(3))})),
                _ => match checkpoints.pop() {
                    Some(checkpoint) if next(2) == 0 => ("edit.restore_checkpoint", json!({"checkpoint": checkpoint})),
                    _ => ("edit.checkpoint", json!({})),
                },
            };
            let r = s.execute(id, &p);
            if id == "edit.checkpoint"
                && let Ok(v) = &r
            {
                checkpoints.push(v["checkpoint"].clone());
            }
            assert!(!matches!(r, Err(crate::EngineError::Internal(..))), "{id} {p}: {r:?}");
            drawing::check_in_design(&s.doc.drawings).unwrap();
            assert!(s.doc.drawings.windows(2).all(|w| (&w[0].name, w[0].id) < (&w[1].name, w[1].id)), "{id}: in their order");
            assert!(s.open_drawing.is_none_or(|id| s.doc.drawing(id).is_some()), "{id}: the drawing open is the design's");
        }
    }
}
