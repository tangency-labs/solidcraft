use super::*;
use crate::cmd::{Part, command_specs, find_command};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"))
}

fn refused(s: &mut Session, id: &str, p: Value) -> String {
    s.execute(id, &p).err().map(|e| e.to_string()).unwrap_or_else(|| panic!("{id} accepted {p}"))
}

/// A folder of the test's own, removed when dropped.
struct Folder(std::path::PathBuf);

impl Folder {
    fn new(name: &str) -> Folder {
        let dir = solidcraft_io::vfs::temp_dir().join(format!("solidcraft-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        Folder(dir)
    }
    fn path(&self, file: &str) -> String {
        self.0.join(file).to_str().unwrap().to_string()
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
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

/// The shared model-store port must not make a saved sheet sketch a model feature,
/// even when their numeric ids coincide. Drawing tools are not enabled by this port.
#[test]
fn sheet_records_do_not_redirect_the_shared_model_sketch_store() {
    use crate::cmd::sketch_store;
    use solidcraft_doc::drawing::SheetSketch;
    use solidcraft_geom::Vec2;
    use solidcraft_sketch::Sketch;

    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Model sketch"}));
    let model_id = s.active_sketch.unwrap();
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [40, 0]]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "drawing.create", json!({"name": "Drawing", "units": "in"}));
    let mut ids = IdSource::seeded(6336);
    let separate = s.drawing("test").unwrap().allocate_id(&mut ids).unwrap();
    assert!(s.doc.feature(separate).is_none());
    s.edit_drawing("test", |drawing| {
        let mut sheet_sketch = Sketch::new();
        sheet_sketch.add_point(Vec2::new(15.0, 10.0), Some("sheet_point"))?;
        for id in [model_id, separate] {
            drawing.sketches.push(Arc::new(SheetSketch {
                id,
                sheet: drawing.sheets[0].id,
                name: "Sheet sketch".into(),
                sketch: sheet_sketch.clone(),
            }));
        }
        Ok(())
    })
    .unwrap();
    let drawing = open_one(&s);
    let before = s.doc.clone();
    let history = (s.undo.len(), s.redo.len(), s.evaluations);
    let model = sketch_store::read(&s.doc, model_id).unwrap();
    assert_eq!(model.curves.len(), 1);
    assert!(model.resolve_point("sheet_point").is_none());
    assert!(sketch_store::read(&s.doc, separate).is_err());
    assert!(sketch_store::target(&s, &json!({"sketch": separate}), "test").is_err());
    for command in ["sketch.edit", "sketch.line", "sketch.export_dxf", "sketch.solve"] {
        let error = refused(&mut s, command, json!({"sketch": model_id, "points": [[0, 0], [10, 0]]}));
        assert!(error.contains("a drawing is open"), "{command}: {error}");
        assert!(Arc::ptr_eq(&s.doc, &before));
        assert_eq!((s.undo.len(), s.redo.len(), s.evaluations), history);
    }
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "sketch.edit", json!({"sketch": model_id.to_string()}));
    run(&mut s, "sketch.point", json!({"point": [60, 5], "id": "model_point"}));
    assert!(sketch_store::read(&s.doc, model_id).unwrap().resolve_point("model_point").is_some());
    assert!(Arc::ptr_eq(s.doc.drawing(drawing.id).unwrap(), &drawing));
    assert!(drawing.sheet_sketch(model_id).unwrap().sketch.resolve_point("model_point").is_none());
}

/// Callback refusal inside the existing store never publishes its private clone.
#[test]
fn receiver_store_callback_failure_preserves_model_drawings_and_history() {
    use crate::cmd::sketch_store;
    use solidcraft_geom::Vec2;

    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    let id = s.active_sketch.unwrap();
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [40, 0]]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "drawing.create", json!({}));
    run(&mut s, "drawing.close", json!({}));
    let before = s.doc.clone();
    let state = s.model.state();
    let history = (s.undo.len(), s.redo.len(), s.evaluations);
    let result: Result<()> = sketch_store::update(&mut s, id, |_, _, sketch| {
        sketch.add_point(Vec2::new(15.0, 10.0), Some("not_committed"))?;
        Err(bad("test", "refuse after changing the private sketch"))
    });
    assert!(result.unwrap_err().to_string().contains("private sketch"));
    assert!(Arc::ptr_eq(&s.doc, &before));
    assert!(Arc::ptr_eq(&s.model.state(), &state));
    assert_eq!((s.undo.len(), s.redo.len(), s.evaluations), history);
    assert!(sketch_store::read(&s.doc, id).unwrap().resolve_point("not_committed").is_none());
}

// W38a R4: exercise real commands and Session history, not an alternative sketcher.
fn sheet_editor() -> (Session, u64, u64, Value) {
    let mut s = design();
    s.doc_mut().units = "in".into();
    let drawing = run(&mut s, "drawing.create", json!({"name": "Paper", "units": "in"}));
    let owner = drawing["sheets"][0]["id"].clone();
    let made = run(&mut s, "drawing.sketch.create", json!({"owner": owner, "name": "Detail"}));
    let d = ids::decode(made["drawing"].as_str().unwrap()).unwrap();
    let sk = ids::decode(made["sketch"].as_str().unwrap()).unwrap();
    (s, d, sk, made)
}

#[test]
fn sheet_editor_existing_tools_make_paper_geometry_without_evaluating_the_model() {
    use solidcraft_geom::Vec2;
    let (mut s, d, sk, made) = sheet_editor();
    let source = s.doc.clone();
    let model = s.model.state();
    let count = s.evaluations;
    let steps = s.undo.len();
    assert!(s.active_sketch.is_none(), "a sheet id is never a model active_sketch");
    assert_eq!(s.active_sheet_sketch.as_ref().unwrap().sketch, sk);
    let rectangle = run(&mut s, "sketch.rectangle.two_point", json!({"p0": [15,10], "p1": [55,30]}));
    assert_eq!(rectangle["created"]["curves"].as_array().unwrap().len(), 4);
    let record = s.doc.drawing(d).unwrap().sheet_sketch(sk).unwrap();
    assert_eq!(record.sketch.curves.len(), 4);
    for at in [[15.0, 10.0], [55.0, 10.0], [55.0, 30.0], [15.0, 30.0]] {
        assert!(record.sketch.points.iter().any(|p| p.pos.dist(Vec2::new(at[0], at[1])) < 1e-8));
    }
    run(&mut s, "sketch.line", json!({"points": [[70,10],[100,50]]}));
    let record = s.doc.drawing(d).unwrap().sheet_sketch(sk).unwrap();
    assert_eq!(record.sketch.curves.len(), 5);
    assert_eq!(s.undo.len(), steps + 2);
    assert_eq!(s.undo.last().unwrap().part, Some(d));
    assert_eq!((s.evaluations, &s.doc.features, &s.doc.params), (count, &source.features, &source.params));
    assert!(Arc::ptr_eq(&model, &s.model.state()));
    let inspected = run(&mut s, "drawing.sketch.inspect", json!({}));
    assert_eq!(inspected["sketch"], made["sketch"]);
    assert_eq!(inspected["curves"].as_array().unwrap().len(), 5);
    assert_eq!(s.undo.len(), steps + 2, "inspection adds no step");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.drawing(d).unwrap().sheet_sketch(sk).unwrap().sketch.curves.len(), 4);
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(s.doc.drawing(d).unwrap().sheet_sketch(sk).unwrap().sketch.curves.len(), 5);
    assert_eq!(s.evaluations, count);
}

#[test]
fn sheet_editor_point_and_line_expressions_keep_paper_defaults_and_read_only_source() {
    let (mut s, d, sk, _) = sheet_editor();
    s.doc_mut().set_param("Inset", "1", Some("in"), None).unwrap();
    s.doc_mut().set_param("Angle", "90", Some("deg"), None).unwrap();
    let original = s.doc.clone();
    let count = s.evaluations;
    run(&mut s, "sketch.point", json!({"point": ["Inset + 5", "2"], "id": "driven"}));
    run(&mut s, "sketch.line", json!({"points": [["Inset", "5"], ["Inset + 10", "5"]]}));
    let sketch = &s.doc.drawing(d).unwrap().sheet_sketch(sk).unwrap().sketch;
    let point = &sketch.points[sketch.resolve_point("driven").unwrap()];
    assert!((point.pos.x - 30.4).abs() < 1e-9);
    assert_eq!(point.pos.y, 2.0);
    assert_eq!(point.exprs.as_ref().unwrap().x.as_deref(), Some("Inset + 5"));
    assert!(sketch.points.iter().any(|p| (p.pos.x - 25.4).abs() < 1e-9 && p.pos.y == 5.0));
    assert!(sketch.points.iter().any(|p| (p.pos.x - 35.4).abs() < 1e-9 && p.pos.y == 5.0));
    assert_eq!((&s.doc.params, &s.doc.features), (&original.params, &original.features));
    for point in [json!(["Missing", 0]), json!(["Angle", 0]), json!(["1 / 0", 0])] {
        let before = s.doc.clone();
        let entry = s.active_sheet_sketch.clone();
        let history = (s.undo.len(), s.redo.len());
        refused(&mut s, "sketch.point", json!({"point": point}));
        assert!(Arc::ptr_eq(&before, &s.doc));
        assert_eq!(s.active_sheet_sketch, entry);
        assert_eq!((s.undo.len(), s.redo.len()), history);
    }
    assert_eq!(s.evaluations, count);
}

#[test]
fn sheet_editor_finish_cancel_and_their_undo_preserve_the_correct_entry() {
    let (mut s, d, sk, made) = sheet_editor();
    let count = s.evaluations;
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [15,10], "p1": [55,30]}));
    let finished = s.doc.drawing(d).unwrap().sheet_sketch(sk).unwrap().clone();
    assert_eq!(run(&mut s, "sketch.finish", json!({}))["editing"], false);
    assert!(s.active_sheet_sketch.is_none());
    run(&mut s, "drawing.sketch.edit", json!({"sketch": made["sketch"]}));
    run(&mut s, "sketch.point", json!({"point": [90,60], "id": "temporary"}));
    let edited = s.doc.drawing(d).unwrap().sheet_sketch(sk).unwrap().clone();
    // Re-entering the same sketch must not move the cancellation boundary.
    run(&mut s, "drawing.sketch.edit", json!({"sketch": made["sketch"]}));
    assert_eq!(run(&mut s, "sketch.cancel", json!({}))["removed"], false);
    assert!(Arc::ptr_eq(&finished, s.doc.drawing(d).unwrap().sheet_sketch(sk).unwrap()));
    assert!(s.active_sheet_sketch.is_none());
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.drawing(d).unwrap().sheet_sketch(sk).unwrap().sketch, edited.sketch);
    assert_eq!(s.active_sheet_sketch.as_ref().unwrap().sketch, sk);
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(s.doc.drawing(d).unwrap().sheet_sketch(sk).unwrap().sketch, finished.sketch);
    assert!(s.active_sheet_sketch.is_none());
    let sheet = made["sheet"].clone();
    let new = run(&mut s, "drawing.sketch.create", json!({"owner": sheet}));
    let new_id = ids::decode(new["sketch"].as_str().unwrap()).unwrap();
    run(&mut s, "sketch.point", json!({"point": [5,7]}));
    assert_eq!(run(&mut s, "sketch.cancel", json!({}))["removed"], true);
    assert!(s.doc.drawing(d).unwrap().sheet_sketch(new_id).is_none());
    assert_eq!(s.doc.drawing(d).unwrap().sheet_sketch(sk).unwrap().sketch, finished.sketch);
    run(&mut s, "edit.undo", json!({}));
    assert!(s.doc.drawing(d).unwrap().sheet_sketch(new_id).is_some());
    run(&mut s, "edit.redo", json!({}));
    assert!(s.doc.drawing(d).unwrap().sheet_sketch(new_id).is_none());
    assert!(s.active_sheet_sketch.is_none());
    assert_eq!(s.evaluations, count);
}

#[test]
fn sheet_editor_checkpoint_scratch_and_reopen_keep_owners_separate() {
    let (mut s, d, sk, made) = sheet_editor();
    let count = s.evaluations;
    let checkpoint = run(&mut s, "edit.checkpoint", json!({}))["checkpoint"].clone();
    let before = s.doc.clone();
    let mut scratch = s.scratch();
    run(&mut scratch, "sketch.rectangle.two_point", json!({"p0": [15,10], "p1": [55,30]}));
    assert!(Arc::ptr_eq(&before, &s.doc));
    assert!(s.doc.drawing(d).unwrap().sheet_sketch(sk).unwrap().sketch.curves.is_empty());
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [15,10], "p1": [55,30]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "edit.restore_checkpoint", json!({"checkpoint": checkpoint}));
    assert!(s.doc.drawing(d).unwrap().sheet_sketch(sk).unwrap().sketch.curves.is_empty());
    assert_eq!(s.active_sheet_sketch.as_ref().unwrap().sketch, sk);
    run(&mut s, "sketch.line", json!({"points": [[15,10],[55,30]]}));
    run(&mut s, "sketch.finish", json!({}));
    let saved = s.doc.to_json();
    let document = solidcraft_doc::Document::from_json(&saved).unwrap();
    let mut reopened = Session::new(document);
    assert!(reopened.active_sheet_sketch.is_none());
    run(&mut reopened, "drawing.open", json!({"drawing": made["drawing"]}));
    run(&mut reopened, "drawing.sketch.edit", json!({"sketch": made["sketch"]}));
    let result = run(&mut reopened, "drawing.sketch.inspect", json!({}));
    assert_eq!(result["curves"].as_array().unwrap().len(), 1);
    assert_eq!(reopened.doc.to_json(), saved);
    assert_eq!(s.evaluations, count);
}

#[test]
fn sheet_editor_rejects_cross_targets_model_tools_and_source_mutation() {
    let (mut s, d, sk, made) = sheet_editor();
    let before = s.doc.clone();
    let entry = s.active_sheet_sketch.clone();
    let state = s.model.state();
    let history = (s.undo.len(), s.redo.len(), s.evaluations);
    let model = s.doc.features[0].id;
    assert_ne!(model, sk);
    for (command, params) in [
        ("sketch.point", json!({"sketch": model, "point": [2,3]})),
        ("sketch.point", json!({"sketch": -1, "point": [2,3]})),
        ("sketch.point", json!({"sketch": 1.5, "point": [2,3]})),
        ("sketch.line", json!({"points": ["vertex:1,2,3",[4,5]]})),
        ("sketch.dimension", json!({"entities": [], "value": "Forbidden = 3"})),
        ("sketch.project", json!({"refs": []})),
        ("sketch.create", json!({"plane": "XY"})),
        ("drawing.close", json!({})),
        ("drawing.open", json!({"drawing": made["drawing"]})),
        ("drawing.delete", json!({"drawing": made["drawing"]})),
        ("drawing.sketch.create", json!({"owner": made["sheet"]})),
    ] {
        refused(&mut s, command, params);
        assert!(Arc::ptr_eq(&s.doc, &before), "{command}");
        assert_eq!(s.active_sheet_sketch, entry, "{command}");
        assert_eq!((s.undo.len(), s.redo.len(), s.evaluations), history, "{command}");
    }
    let result = super::super::sketch_store::edit_sheet(&mut s, &json!({}), "test", |_sketch, doc| {
        doc.name = "Forbidden mutation".into();
        Ok(())
    });
    assert!(result.unwrap_err().to_string().contains("cannot change the source design"));
    assert!(Arc::ptr_eq(&s.doc, &before) && Arc::ptr_eq(&s.model.state(), &state));
    assert_eq!(s.open_drawing, Some(d));
}

#[test]
fn sheet_editor_new_commands_validate_hostile_inputs_and_declare_schemas() {
    let mut s = design();
    let drawing = run(&mut s, "drawing.create", json!({}));
    let before = s.doc.clone();
    let history = (s.undo.len(), s.redo.len(), s.evaluations);
    for params in [
        json!({}),
        json!({"owner": 1}),
        json!({"owner": "not-an-id"}),
        json!({"owner": drawing["id"]}),
        json!({"owner": drawing["sheets"][0]["id"], "name": " "}),
        json!({"owner": drawing["sheets"][0]["id"], "name": "X".repeat(257)}),
        json!({"owner": drawing["sheets"][0]["id"], "unexpected": true}),
        json!([]),
    ] {
        refused(&mut s, "drawing.sketch.create", params);
        assert!(Arc::ptr_eq(&s.doc, &before));
        assert!(s.active_sheet_sketch.is_none());
        assert_eq!((s.undo.len(), s.redo.len(), s.evaluations), history);
    }
    for command in ["drawing.sketch.create", "drawing.sketch.edit", "drawing.sketch.inspect", "sketch.cancel"] {
        let spec = find_command(command).unwrap();
        assert!(spec.contract.is_some() && spec.output.is_some(), "{command}");
    }
    s.param_check = crate::schema::ParamCheck::Strict;
    let made = run(&mut s, "drawing.sketch.create", json!({"owner": drawing["sheets"][0]["id"]}));
    assert!(made["editing"].as_bool().unwrap());
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "drawing.sketch.edit", json!({"sketch": made["sketch"]}));
    run(&mut s, "sketch.cancel", json!({}));
}

#[test]
fn sheet_editor_same_numeric_id_in_two_drawings_and_model_never_retargets_geometry() {
    use solidcraft_doc::drawing::SheetSketch;
    use solidcraft_sketch::Sketch;
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Model"}));
    let id = s.active_sketch.unwrap();
    run(&mut s, "sketch.line", json!({"points": [[0,0],[40,0]]}));
    run(&mut s, "sketch.finish", json!({}));
    let model = s.doc.sketch(id).unwrap().clone();
    run(&mut s, "drawing.create", json!({"name": "First"}));
    s.edit_drawing("test", |drawing| {
        drawing.sketches.push(Arc::new(SheetSketch { id, sheet: drawing.sheets[0].id, name: "Same id".into(), sketch: Sketch::new() }));
        Ok(())
    })
    .unwrap();
    let first = open_one(&s);
    let mut second = (*first).clone();
    second.id = first.allocate_id(&mut IdSource::seeded(7082)).unwrap();
    second.name = "Second".into();
    let second_id = second.id;
    s.doc_mut().put_drawing(Arc::new(second));
    let second = s.doc.drawing(second_id).unwrap().clone();
    let count = s.evaluations;
    run(&mut s, "drawing.sketch.edit", json!({"sketch": ids::encode(id)}));
    run(&mut s, "sketch.point", json!({"sketch": id, "point": [15,10], "id": "first_point"}));
    assert_eq!(s.doc.sketch(id).unwrap(), &model);
    assert!(Arc::ptr_eq(s.doc.drawing(second_id).unwrap(), &second));
    run(&mut s, "sketch.finish", json!({}));
    let first_edited = s.doc.drawing(first.id).unwrap().clone();
    run(&mut s, "drawing.open", the(second_id));
    run(&mut s, "drawing.sketch.edit", json!({"sketch": ids::encode(id)}));
    run(&mut s, "sketch.point", json!({"point": [50,20], "id": "second_point"}));
    assert_eq!(s.doc.sketch(id).unwrap(), &model);
    assert!(Arc::ptr_eq(s.doc.drawing(first.id).unwrap(), &first_edited));
    assert!(s.doc.drawing(second_id).unwrap().sheet_sketch(id).unwrap().sketch.resolve_point("first_point").is_none());
    run(&mut s, "edit.undo", json!({}));
    assert!(s.doc.drawing(second_id).unwrap().sheet_sketch(id).unwrap().sketch.resolve_point("second_point").is_none());
    assert!(Arc::ptr_eq(s.doc.drawing(first.id).unwrap(), &first_edited));
    assert_eq!(s.evaluations, count);
}

#[test]
fn sheet_editor_commands_and_refusals_do_not_evaluate_a_stale_model() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    let model_sketch = s.active_sketch.unwrap();
    let line = run(&mut s, "sketch.line", json!({"points": [[0,0],[40,0]]}))["curves"][0].as_str().unwrap().to_string();
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.box", json!({"length": 5,"width": 5,"height": 5}));
    run(&mut s, "sketch.edit", json!({"sketch": model_sketch}));
    run(&mut s, "sketch.drag", json!({"entity": format!("{line}.end"), "from": [40,0], "to": [45,3], "live": true}));
    assert!(s.model.is_stale(), "the fixture includes an unfinished model gesture");
    let model = s.model.state();
    let count = s.evaluations;
    let drawing = run(&mut s, "drawing.create", json!({}));
    run(&mut s, "drawing.sketch.create", json!({"owner": drawing["sheets"][0]["id"]}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [15,10], "p1": [55,30]}));
    run(&mut s, "drawing.sketch.inspect", json!({}));
    refused(&mut s, "solid.box", json!({"length": 3,"width": 3,"height": 3}));
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.redo", json!({}));
    run(&mut s, "sketch.finish", json!({}));
    assert!(s.model.is_stale());
    assert!(Arc::ptr_eq(&model, &s.model.state()));
    assert_eq!(s.evaluations, count);
    assert_eq!(s.active_sketch, Some(model_sketch), "model active state was not overwritten");
}

// W38a review: an unchanged Arc is not a validation certificate.
#[test]
fn sheet_review_publication_revalidates_unchanged_sketch_records() {
    let (mut s, d, sk, _) = sheet_editor();
    let owner = s.doc.drawing(d).unwrap().clone();
    let mut next = (*owner).clone();
    let mut other = (*next.sheet_sketch(sk).unwrap().as_ref()).clone();
    other.id = next.allocate_id(&mut IdSource::seeded(7608)).unwrap();
    other.name = "Unchanged invalid payload".into();
    other.sketch.add_point(solidcraft_geom::Vec2::new(4.0, 5.0), Some("invalid_point")).unwrap();
    other.sketch.points[1].pos.x = solidcraft_sketch::MAX_COORD;
    let invalid = Arc::new(other);
    next.sketches.push(invalid.clone());
    s.doc_mut().put_drawing(Arc::new(next));
    let before = s.doc.clone();
    let model = s.model.state();
    let history = (s.undo.len(), s.redo.len(), s.evaluations);
    let e = s.execute("sketch.point", &json!({"point":[12,13],"id":"must_not_publish"})).unwrap_err().to_string();
    assert!(e.contains("out of range"), "{e}");
    assert!(Arc::ptr_eq(&s.doc, &before));
    assert!(Arc::ptr_eq(s.doc.drawing(d).unwrap().sheet_sketch(invalid.id).unwrap(), &invalid));
    assert!(s.doc.drawing(d).unwrap().sheet_sketch(sk).unwrap().sketch.resolve_point("must_not_publish").is_none());
    assert_eq!((s.undo.len(), s.redo.len(), s.evaluations), history);
    assert!(Arc::ptr_eq(&s.model.state(), &model));
}

#[test]
fn sheet_review_shared_store_noop_keeps_the_document_and_record_arcs() {
    let (mut s, d, sk, _) = sheet_editor();
    let before = s.doc.clone();
    let record = before.drawing(d).unwrap().sheet_sketch(sk).unwrap().clone();
    let history = (s.undo.len(), s.redo.len(), s.evaluations);
    let (answer, _) = crate::cmd::sketch_store::edit_sheet(&mut s, &json!({}), "test", |_, _| Ok(42)).unwrap();
    assert_eq!(answer, 42);
    assert!(Arc::ptr_eq(&s.doc, &before));
    assert!(Arc::ptr_eq(s.doc.drawing(d).unwrap().sheet_sketch(sk).unwrap(), &record));
    assert_eq!((s.undo.len(), s.redo.len(), s.evaluations), history);
}

// W38a R6: existing 2D constructors use the sheet receiver and never its model's snaps.
#[test]
fn sheet_next_snap_directives_cannot_modify_an_active_model_sketch() {
    use solidcraft_geom::Vec2;
    let mut s = design();
    run(&mut s, "sketch.create", json!({"plane":"XY"}));
    let model_id = s.active_sketch.unwrap();
    let line = run(&mut s, "sketch.line", json!({"points":[[0,0],[20,0]]}))["curves"][0].as_str().unwrap().to_string();
    // Deliberately collide with the point id a model midpoint snap would allocate.
    let mut model_copy = s.doc.sketch(model_id).unwrap().clone();
    let at = model_copy.add_point(Vec2::new(10.0, 0.0), None).unwrap();
    let same_local_id = model_copy.points[at].id.clone();
    let drawing = run(&mut s, "drawing.create", json!({"name":"Sheet with an active model sketch"}));
    run(&mut s, "drawing.sketch.create", json!({"owner":drawing["sheets"][0]["id"]}));
    run(&mut s, "sketch.point", json!({"point":[31,43], "id":same_local_id}));
    let before = s.doc.clone();
    let state = s.model.state();
    let entry = s.active_sheet_sketch.clone();
    let history = (s.undo.len(), s.redo.len(), s.evaluations);
    for marker in [
        json!(format!("mid:{line}")),
        json!(format!("on:{line}:10,0")),
        json!("vertex:0,0,0"),
        json!({"vertex":[0,0,0]}),
        json!({"on_edge":[10,0,0]}),
        json!({"vertex":false,"at":[31,43]}),
    ] {
        for (command, params) in [
            ("sketch.line", json!({"points":[marker.clone(),[55,43]]})),
            ("sketch.circle.center", json!({"center":marker.clone(),"radius":5})),
            ("sketch.arc.center_point", json!({"center":marker.clone(),"start":[36,43],"sweep":90})),
            ("sketch.arc.three_point", json!({"start":marker.clone(),"end":[41,43],"through":[36,48]})),
        ] {
            let error = refused(&mut s, command, params);
            assert!(error.contains("not supported yet"), "{command}: {error}");
            assert!(Arc::ptr_eq(&s.doc, &before));
            assert!(Arc::ptr_eq(&s.model.state(), &state));
            assert_eq!(s.active_sheet_sketch, entry);
            assert_eq!(s.active_sketch, Some(model_id));
            assert_eq!((s.undo.len(), s.redo.len(), s.evaluations), history);
        }
    }
}

#[test]
fn sheet_next_circles_keep_analytic_centres_radii_and_paper_units() {
    use solidcraft_geom::Vec2;
    use solidcraft_sketch::CurveKind;
    for (command, params) in [
        ("sketch.circle.center", json!({"center":[30,40],"radius":5})),
        ("sketch.circle.center", json!({"center":[30,40],"diameter":10})),
        ("sketch.circle.two_point", json!({"p0":[25,40],"p1":[35,40]})),
        ("sketch.circle.three_point", json!({"p0":[35,40],"p1":[30,45],"p2":[25,40]})),
    ] {
        let (mut s, d, sk, _) = sheet_editor();
        let before = s.doc.clone();
        let state = s.model.state();
        let history = (s.undo.len(), s.evaluations);
        let made = run(&mut s, command, params);
        let record = s.doc.drawing(d).unwrap().sheet_sketch(sk).unwrap();
        assert_eq!(record.sketch.curves.len(), 1);
        let CurveKind::Circle { c, r } = record.sketch.curves[0].kind else { panic!("{command}: not a circle") };
        assert!(record.sketch.points[c].pos.dist(Vec2::new(30.0, 40.0)) < 1e-9, "{command}");
        assert!((r - 5.0).abs() < 1e-9, "{command}");
        assert_eq!(made["created"]["curves"].as_array().unwrap().len(), 1);
        assert_eq!((&s.doc.features, &s.doc.params, s.evaluations), (&before.features, &before.params, history.1));
        assert!(Arc::ptr_eq(&state, &s.model.state()));
        assert_eq!(s.undo.len(), history.0 + 1);
        assert_eq!(s.undo.last().unwrap().part, Some(d));
        run(&mut s, "edit.undo", json!({}));
        assert!(s.doc.drawing(d).unwrap().sheet_sketch(sk).unwrap().sketch.curves.is_empty());
        run(&mut s, "edit.redo", json!({}));
        assert_eq!(s.doc.drawing(d).unwrap().sheet_sketch(sk).unwrap().sketch.curves.len(), 1);
    }
}

#[test]
fn sheet_next_arcs_keep_signed_direction_and_analytic_endpoints() {
    use solidcraft_geom::Vec2;
    use solidcraft_sketch::CurveKind;
    for (command, params, end, reversed) in [
        ("sketch.arc.center_point", json!({"center":[30,40],"start":[35,40],"sweep":90}), [30.0, 45.0], false),
        ("sketch.arc.center_point", json!({"center":[30,40],"start":[35,40],"sweep":-90}), [30.0, 35.0], true),
        ("sketch.arc.center_point", json!({"center":[30,40],"start":[35,40],"end":[30,45]}), [30.0, 45.0], false),
        ("sketch.arc.three_point", json!({"start":[35,40],"end":[25,40],"through":[30,45]}), [25.0, 40.0], false),
        ("sketch.arc.three_point", json!({"start":[35,40],"end":[25,40],"through":[30,35]}), [25.0, 40.0], true),
    ] {
        let (mut s, d, sk, _) = sheet_editor();
        let before = s.doc.clone();
        let evaluations = s.evaluations;
        run(&mut s, command, params);
        let record = s.doc.drawing(d).unwrap().sheet_sketch(sk).unwrap();
        let curve = &record.sketch.curves[0];
        let CurveKind::Arc { c, a, b } = curve.kind else { panic!("{command}: not an arc") };
        let point = |index: usize| record.sketch.points[index].pos;
        let (start, finish) = if curve.reversed { (point(b), point(a)) } else { (point(a), point(b)) };
        assert_eq!(curve.reversed, reversed, "{command}");
        assert!(point(c).dist(Vec2::new(30.0, 40.0)) < 1e-8);
        assert!(start.dist(Vec2::new(35.0, 40.0)) < 1e-8);
        assert!(finish.dist(Vec2::new(end[0], end[1])) < 1e-8);
        assert!((start.dist(point(c)) - 5.0).abs() < 1e-8 && (finish.dist(point(c)) - 5.0).abs() < 1e-8);
        assert_eq!((&s.doc.features, &s.doc.params, s.evaluations), (&before.features, &before.params, evaluations));
        let saved = s.doc.to_json();
        let reopened = solidcraft_doc::Document::from_json(&saved).unwrap();
        assert_eq!(reopened.drawing(d).unwrap().sheet_sketch(sk).unwrap().sketch, record.sketch);
    }
}

#[test]
fn sheet_next_rectangle_variants_keep_their_constraints_and_construction_geometry() {
    use solidcraft_geom::Vec2;
    for (command, params, corners, construction) in [
        ("sketch.rectangle.center", json!({"center":[35,20],"corner":[55,30]}), [[15.0, 10.0], [55.0, 10.0], [55.0, 30.0], [15.0, 30.0]], 2),
        (
            "sketch.rectangle.three_point",
            json!({"p0":[10,10],"p1":[40,50],"p2":[-6,22]}),
            [[10.0, 10.0], [40.0, 50.0], [24.0, 62.0], [-6.0, 22.0]],
            0,
        ),
    ] {
        let (mut s, d, sk, _) = sheet_editor();
        let count = s.evaluations;
        let model = s.model.state();
        run(&mut s, command, params);
        let record = s.doc.drawing(d).unwrap().sheet_sketch(sk).unwrap();
        assert_eq!(record.sketch.curves.iter().filter(|c| !c.construction).count(), 4);
        assert_eq!(record.sketch.curves.iter().filter(|c| c.construction).count(), construction);
        for p in corners {
            assert!(record.sketch.points.iter().any(|q| q.pos.dist(Vec2::new(p[0], p[1])) < 1e-7), "{command}: {p:?}");
        }
        assert!(s.doc.solve_sheet_sketch(d, sk).unwrap().report.keeps_orientation());
        assert!(Arc::ptr_eq(&model, &s.model.state()));
        assert_eq!(count, s.evaluations);
        run(&mut s, "sketch.cancel", json!({}));
        assert!(s.doc.drawing(d).unwrap().sheet_sketch(sk).is_none());
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(s.doc.drawing(d).unwrap().sheet_sketch(sk).unwrap().sketch.curves.len(), 4 + construction);
    }
}

#[test]
fn sheet_next_invalid_round_geometry_is_atomic_and_exact_targeted() {
    let (mut s, _, _, made) = sheet_editor();
    let before = s.doc.clone();
    let entry = s.active_sheet_sketch.clone();
    let model = s.model.state();
    let history = (s.undo.len(), s.redo.len(), s.evaluations);
    for (cmd, params) in [
        ("sketch.circle.center", json!({"center":[10,10],"radius":-2})),
        ("sketch.circle.center", json!({"center":[10,10],"radius":1e100})),
        ("sketch.circle.center", json!({"center":["Absent + 5",10],"radius":2})),
        ("sketch.circle.center", json!({"center":[10,10],"radius":2,"sketch":made["sheet"]})),
        ("sketch.circle.two_point", json!({"p0":[10,10],"p1":[10,10]})),
        ("sketch.circle.three_point", json!({"p0":[10,10],"p1":[20,10],"p2":[30,10]})),
        ("sketch.arc.center_point", json!({"center":[30,40],"start":[35,40],"sweep":360})),
        ("sketch.arc.center_point", json!({"center":[30,40],"start":[35,40],"sweep":0})),
        ("sketch.arc.three_point", json!({"start":[10,10],"end":[30,10],"through":[20,10]})),
        ("sketch.rectangle.center", json!({"center":[10,10],"corner":[10,20]})),
        ("sketch.rectangle.three_point", json!({"p0":[10,10],"p1":[20,10],"p2":[30,10]})),
    ] {
        refused(&mut s, cmd, params);
        assert!(Arc::ptr_eq(&s.doc, &before) && Arc::ptr_eq(&s.model.state(), &model), "{cmd}");
        assert_eq!(s.active_sheet_sketch, entry);
        assert_eq!((s.undo.len(), s.redo.len(), s.evaluations), history);
    }
}

#[test]
fn sheet_next_accept_computed_adopts_the_open_owner_entry_and_cancel_boundary() {
    let mut s = design();
    let a = run(&mut s, "drawing.create", json!({"name":"A"}));
    run(&mut s, "drawing.close", json!({}));
    let b = run(&mut s, "drawing.create", json!({"name":"B"}));
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "drawing.open", json!({"drawing":a["id"]}));
    let checkpoint = run(&mut s, "edit.checkpoint", json!({}))["checkpoint"].clone();
    s.path = Some("computed-sheet.synthetic.solidcraft".into());
    let before = s.doc.clone();
    let model = s.model.state();
    let steps = s.undo.len();
    let evaluations = s.evaluations;
    let mut computed = s.scratch();
    run(&mut computed, "drawing.open", json!({"drawing":b["id"]}));
    let made = run(&mut computed, "drawing.sketch.create", json!({"owner":b["sheets"][0]["id"]}));
    run(&mut computed, "sketch.rectangle.two_point", json!({"p0":[15,10],"p1":[55,30]}));
    let drawing = ids::decode(b["id"].as_str().unwrap()).unwrap();
    let sketch = ids::decode(made["sketch"].as_str().unwrap()).unwrap();
    assert!(Arc::ptr_eq(&before, &s.doc));
    assert_ne!(s.open_drawing, computed.open_drawing);
    let entry = computed.active_sheet_sketch.clone();
    let additional_steps = computed.undo.len();
    s.accept_computed(computed);
    assert_eq!(s.open_drawing, Some(drawing));
    assert_eq!(s.active_sheet_sketch, entry);
    assert_eq!(s.path.as_deref(), Some("computed-sheet.synthetic.solidcraft"));
    assert_eq!(s.undo.len(), steps + additional_steps);
    assert!(s.undo[steps..].iter().all(|step| step.part == Some(drawing)));
    assert_eq!(s.evaluations, evaluations);
    assert!(Arc::ptr_eq(&model, &s.model.state()));
    assert_eq!(s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch.curves.len(), 4);
    run(&mut s, "sketch.cancel", json!({}));
    assert!(s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).is_none());
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.active_sheet_sketch, entry);
    assert_eq!(s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch.curves.len(), 4);
    let mut closed = s.scratch();
    run(&mut closed, "sketch.finish", json!({}));
    run(&mut closed, "drawing.close", json!({}));
    s.accept_computed(closed);
    assert!(s.open_drawing.is_none() && s.active_sheet_sketch.is_none());
    run(&mut s, "edit.restore_checkpoint", json!({"checkpoint":checkpoint}));
    assert_eq!(s.open_drawing, Some(ids::decode(a["id"].as_str().unwrap()).unwrap()));
    assert!(s.active_sheet_sketch.is_none());
    assert_eq!(s.doc.to_json(), before.to_json());
}

#[test]
fn sheet_next_curved_point_expressions_use_paper_defaults_and_keep_source_read_only() {
    use solidcraft_geom::Vec2;
    use solidcraft_sketch::CurveKind;
    let mut s = design();
    s.doc_mut().units = "in".into();
    s.doc_mut().set_param("Inset", "1", Some("in"), None).unwrap();
    let d = run(&mut s, "drawing.create", json!({"units":"in"}));
    let made = run(&mut s, "drawing.sketch.create", json!({"owner":d["sheets"][0]["id"]}));
    let drawing = ids::decode(made["drawing"].as_str().unwrap()).unwrap();
    let sketch = ids::decode(made["sketch"].as_str().unwrap()).unwrap();
    let source = s.doc.clone();
    let count = s.evaluations;
    run(&mut s, "sketch.circle.center", json!({"center":["Inset + 5","10"],"radius":5,"id":"circle"}));
    run(&mut s, "sketch.arc.center_point", json!({"center":"circle.center","start":["Inset + 10","10"],"sweep":90,"id":"arc"}));
    let record = s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap();
    let CurveKind::Circle { c, r } = record.sketch.curves[0].kind else { panic!("missing circle") };
    assert!(record.sketch.points[c].pos.dist(Vec2::new(30.4, 10.0)) < 1e-8);
    assert!((r - 5.0).abs() < 1e-8);
    assert_eq!(record.sketch.points[c].exprs.as_ref().unwrap().x.as_deref(), Some("Inset + 5"));
    let CurveKind::Arc { c: arc_center, a, b } = record.sketch.curves[1].kind else { panic!("missing arc") };
    assert_eq!(c, arc_center, "the exact local centre id remains shared");
    assert!(record.sketch.points[a].pos.dist(Vec2::new(35.4, 10.0)) < 1e-8);
    assert!(record.sketch.points[b].pos.dist(Vec2::new(30.4, 15.0)) < 1e-8);
    assert_eq!((&s.doc.features, &s.doc.params, s.evaluations), (&source.features, &source.params, count));
    let saved = s.doc.to_json();
    let reopened = solidcraft_doc::Document::from_json(&saved).unwrap();
    assert_eq!(reopened.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch, record.sketch);
}

// W38a R7: reuse the existing polygon and slot constructors as paper-sketch tools.
#[test]
fn sheet_next_polygon_commands_keep_analytic_paper_geometry_and_history() {
    use solidcraft_geom::Vec2;
    use solidcraft_sketch::CurveKind;
    // Hand-derived vertices. A circumscribed square with apothem 10 has
    // vertices at (+/-10, +/-10); the edge-defined square has centre (40,30).
    let cases = [
        (
            "sketch.polygon.inscribed",
            json!({"center":[60,45],"radius":20,"sides":4,"angle":0}),
            Vec2::new(60.0, 45.0),
            20.0,
            [[80.0, 45.0], [60.0, 65.0], [40.0, 45.0], [60.0, 25.0]],
        ),
        (
            "sketch.polygon.circumscribed",
            json!({"center":[60,45],"radius":10,"sides":4,"angle":0}),
            Vec2::new(60.0, 45.0),
            10.0,
            [[70.0, 55.0], [50.0, 55.0], [50.0, 35.0], [70.0, 35.0]],
        ),
        (
            "sketch.polygon.edge",
            json!({"p0":[30,20],"p1":[50,20],"sides":4}),
            Vec2::new(40.0, 30.0),
            10.0_f64 * 2.0_f64.sqrt(),
            [[30.0, 20.0], [50.0, 20.0], [50.0, 40.0], [30.0, 40.0]],
        ),
    ];
    for (command, params, centre, radius, vertices) in cases {
        let (mut s, d, sk, made) = sheet_editor();
        let before = s.doc.clone();
        let model = s.model.state();
        let (undo, evaluated) = (s.undo.len(), s.evaluations);
        let out = run(&mut s, command, params);
        let record = s.doc.drawing(d).unwrap().sheet_sketch(sk).unwrap();
        let support = out["support_circle"].as_str().unwrap();
        assert_eq!(record.sketch.curves.len(), 5, "{command}");
        let created = out["created"]["curves"].as_array().unwrap();
        assert_eq!(created.len(), 5, "{command}: four sides plus construction guide");
        assert!(created.iter().any(|id| id.as_str() == Some(support)), "{command}: construction circle is reported");
        let lines = record.sketch.curves.iter().filter(|c| matches!(c.kind, CurveKind::Line { .. })).count();
        assert_eq!(lines, 4, "{command}");
        let circle = record.sketch.curves.iter().find(|c| c.id == support).unwrap();
        assert!(circle.construction, "{command}: the radius guide is construction");
        let CurveKind::Circle { c, r } = circle.kind else { panic!("{command}: missing support circle") };
        assert!(record.sketch.points[c].pos.dist(centre) < 1e-7, "{command}: centre");
        assert!((r - radius).abs() < 1e-7, "{command}: radius");
        for at in vertices {
            assert!(record.sketch.points.iter().any(|p| p.pos.dist(Vec2::new(at[0], at[1])) < 1e-7), "{command}: {at:?}");
        }
        assert_eq!((&s.doc.features, &s.doc.params), (&before.features, &before.params));
        assert_eq!(s.evaluations, evaluated);
        assert!(Arc::ptr_eq(&model, &s.model.state()));
        assert_eq!((s.undo.len(), s.undo.last().unwrap().part), (undo + 1, Some(d)));
        run(&mut s, "edit.undo", json!({}));
        assert!(s.doc.drawing(d).unwrap().sheet_sketch(sk).unwrap().sketch.curves.is_empty());
        run(&mut s, "edit.redo", json!({}));
        assert_eq!(s.doc.drawing(d).unwrap().sheet_sketch(sk).unwrap().sketch.curves.len(), 5);
        run(&mut s, "sketch.finish", json!({}));
        let saved = s.doc.to_json();
        let mut reopened = Session::new(solidcraft_doc::Document::from_json(&saved).unwrap());
        run(&mut reopened, "drawing.open", json!({"drawing":made["drawing"]}));
        run(&mut reopened, "drawing.sketch.edit", json!({"sketch":made["sketch"]}));
        assert_eq!(run(&mut reopened, "drawing.sketch.inspect", json!({}))["curves"].as_array().unwrap().len(), 5);
        assert_eq!(reopened.doc.to_json(), saved);
    }
}

#[test]
fn sheet_next_slots_keep_tangent_caps_and_drawing_local_undo() {
    use solidcraft_geom::Vec2;
    use solidcraft_sketch::CurveKind;
    for (command, params, centres) in [
        ("sketch.slot.center_to_center", json!({"p0":[30,30],"p1":[70,30],"width":10}), [Vec2::new(30.0, 30.0), Vec2::new(70.0, 30.0)]),
        ("sketch.slot.overall", json!({"p0":[30,30],"p1":[90,30],"width":10}), [Vec2::new(35.0, 30.0), Vec2::new(85.0, 30.0)]),
    ] {
        let (mut s, d, sk, _) = sheet_editor();
        let before = s.doc.clone();
        let model = s.model.state();
        let (undo, evaluated) = (s.undo.len(), s.evaluations);
        let result = run(&mut s, command, params);
        let sketch = &s.doc.drawing(d).unwrap().sheet_sketch(sk).unwrap().sketch;
        assert_eq!(result["created"]["curves"].as_array().unwrap().len(), 4);
        assert_eq!(sketch.curves.len(), 4, "{command}");
        assert_eq!(sketch.curves.iter().filter(|c| matches!(c.kind, CurveKind::Line { .. })).count(), 2);
        let arcs: Vec<_> = sketch
            .curves
            .iter()
            .filter_map(|c| match c.kind {
                CurveKind::Arc { c: centre, a, b } => Some((sketch.points[centre].pos, sketch.points[a].pos, sketch.points[b].pos)),
                _ => None,
            })
            .collect();
        assert_eq!(arcs.len(), 2);
        for centre in centres {
            let (_, a, b) = arcs.iter().find(|(c, _, _)| c.dist(centre) < 1e-7).expect("expected cap centre");
            assert!((a.dist(centre) - 5.0).abs() < 1e-7);
            assert!((b.dist(centre) - 5.0).abs() < 1e-7);
        }
        assert_eq!((&s.doc.params, &s.doc.features), (&before.params, &before.features));
        assert!(Arc::ptr_eq(&model, &s.model.state()));
        assert_eq!(s.evaluations, evaluated);
        assert_eq!((s.undo.len(), s.undo.last().unwrap().part), (undo + 1, Some(d)));
        run(&mut s, "edit.undo", json!({}));
        assert!(s.doc.drawing(d).unwrap().sheet_sketch(sk).unwrap().sketch.curves.is_empty());
        run(&mut s, "edit.redo", json!({}));
        assert_eq!(s.doc.drawing(d).unwrap().sheet_sketch(sk).unwrap().sketch.curves.len(), 4);
    }
}

#[test]
fn sheet_next_polygon_and_slot_rejections_leave_the_source_and_history_untouched() {
    let (mut s, d, sk, made) = sheet_editor();
    let model_id = s.doc.features[0].id;
    let base = s.doc.clone();
    let original = s.active_sheet_sketch.clone();
    let history = (s.undo.len(), s.redo.len(), s.evaluations);
    for (command, params) in [
        ("sketch.polygon.inscribed", json!({"center":[50,50],"radius":0,"sides":4})),
        ("sketch.polygon.circumscribed", json!({"center":[50,50],"radius":10,"sides":2})),
        ("sketch.polygon.edge", json!({"p0":[30,30],"p1":[30,30],"sides":4})),
        ("sketch.polygon.edge", json!({"p0":[30,30],"p1":[50,30],"sides":201})),
        ("sketch.slot.center_to_center", json!({"p0":[30,30],"p1":[70,30],"width":0})),
        ("sketch.slot.overall", json!({"p0":[30,30],"p1":[50,30],"width":20})),
        ("sketch.slot.overall", json!({"p0":[30,30],"p1":[70,30],"width":-1})),
        ("sketch.polygon.inscribed", json!({"sketch":model_id,"center":[50,50],"radius":10,"sides":4})),
        ("sketch.slot.center_to_center", json!({"sketch":model_id,"p0":[30,30],"p1":[70,30],"width":10})),
    ] {
        refused(&mut s, command, params);
        assert!(Arc::ptr_eq(&s.doc, &base), "{command}");
        assert_eq!(s.active_sheet_sketch, original, "{command}");
        assert_eq!((s.undo.len(), s.redo.len(), s.evaluations), history, "{command}");
    }
    assert_eq!(s.open_drawing, Some(d));
    assert_eq!(s.doc.drawing(d).unwrap().sheet_sketch(sk).unwrap().sketch.curves.len(), 0);
    assert_eq!(run(&mut s, "drawing.sketch.inspect", json!({}))["sketch"], made["sketch"]);
}

// W38a R8: the six point-parser sites not covered by R6's inch drawing.
#[test]
fn sheet_reviewer_curved_parser_sites_use_paper_mm_and_keep_expressions() {
    use solidcraft_geom::Vec2;
    let cases = [
        (
            "circle through",
            "sketch.circle.center",
            json!({"center":[25.4,10], "radius":5, "through":["Inset + 5","10"],"id":"curve"}),
            Vec2::new(30.4, 10.0),
        ),
        (
            "three point arc start",
            "sketch.arc.three_point",
            json!({"start":["Inset + 5","10"],"end":[45,10],"through":[38,17],"id":"curve"}),
            Vec2::new(30.4, 10.0),
        ),
        (
            "three point arc end",
            "sketch.arc.three_point",
            json!({"start":[20,10],"end":["Inset + 5","10"],"through":[25,17],"id":"curve"}),
            Vec2::new(30.4, 10.0),
        ),
        (
            "three point arc through",
            "sketch.arc.three_point",
            json!({"start":[20,10],"end":[40,10],"through":["Inset + 5","20"],"id":"curve"}),
            Vec2::new(30.4, 20.0),
        ),
        (
            "centre point arc centre",
            "sketch.arc.center_point",
            json!({"center":["Inset + 5","10"],"start":[35.4,10],"end":[30.4,15],"id":"curve"}),
            Vec2::new(30.4, 10.0),
        ),
        (
            "centre point arc end",
            "sketch.arc.center_point",
            json!({"center":[30.4,10],"start":[35.4,10],"end":["Inset + 5","15"],"id":"curve"}),
            Vec2::new(30.4, 15.0),
        ),
    ];
    for (site, cmd, params, expected) in cases {
        let (mut s, drawing, sketch, _) = sheet_editor();
        s.doc_mut().set_param("Inset", "1", Some("in"), None).unwrap();
        let source = s.doc.clone();
        let model = s.model.state();
        let (undo, evaluated) = (s.undo.len(), s.evaluations);
        run(&mut s, cmd, params);
        let saved = s.doc.to_json();
        let record = s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap();
        let placed: Vec<_> = record.sketch.points.iter().filter(|p| p.exprs.as_ref().and_then(|x| x.x.as_deref()) == Some("Inset + 5")).collect();
        assert_eq!(placed.len(), 1, "{site}: the authored expression identifies one point");
        assert!(placed[0].pos.dist(expected) < 1e-7, "{site}: paper-mm expression, got {:?}", placed[0].pos);
        assert_eq!(record.sketch.curves.len(), 1, "{site}");
        assert_eq!((&s.doc.features, &s.doc.params), (&source.features, &source.params), "{site}: source unchanged");
        assert!(Arc::ptr_eq(&model, &s.model.state()), "{site}: no model evaluation");
        assert_eq!(s.evaluations, evaluated, "{site}");
        assert_eq!((s.undo.len(), s.undo.last().unwrap().part), (undo + 1, Some(drawing)), "{site}");
        let reopened = solidcraft_doc::Document::from_json(&saved).unwrap();
        assert_eq!(reopened.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch, record.sketch, "{site}: round trip");
    }
}

// The shared constraint commands operate on local sheet entities, never source features.
#[test]
fn sheet_reviewer_shared_geometric_constraints_keep_drawing_local_history() {
    use solidcraft_sketch::CurveKind;
    let cases: Vec<(&str, Vec<(&str, Value)>, Value)> = vec![
        (
            "sketch.constraint.horizontal_vertical",
            vec![("sketch.line", json!({"points":[[10,10],[50,20]],"ids":["a"]}))],
            json!({"line":"a","mode":"horizontal"}),
        ),
        (
            "sketch.constraint.equal",
            vec![
                ("sketch.circle.center", json!({"center":[20,20],"radius":5,"id":"a"})),
                ("sketch.circle.center", json!({"center":[60,20],"radius":9,"id":"b"})),
            ],
            json!({"a":"a","b":"b"}),
        ),
        (
            "sketch.constraint.parallel",
            vec![("sketch.line", json!({"points":[[10,10],[50,10]],"ids":["a"]})), ("sketch.line", json!({"points":[[10,30],[42,42]],"ids":["b"]}))],
            json!({"a":"a","b":"b"}),
        ),
        (
            "sketch.constraint.perpendicular",
            vec![("sketch.line", json!({"points":[[10,10],[50,10]],"ids":["a"]})), ("sketch.line", json!({"points":[[20,30],[38,65]],"ids":["b"]}))],
            json!({"a":"a","b":"b"}),
        ),
        (
            "sketch.constraint.concentric",
            vec![
                ("sketch.circle.center", json!({"center":[20,20],"radius":5,"id":"a"})),
                ("sketch.circle.center", json!({"center":[60,30],"radius":10,"id":"b"})),
            ],
            json!({"a":"a","b":"b"}),
        ),
        (
            "sketch.constraint.midpoint",
            vec![("sketch.line", json!({"points":[[20,20],[60,20]],"ids":["a"]})), ("sketch.point", json!({"point":[35,30],"id":"marker"}))],
            json!({"point":"marker","line":"a"}),
        ),
        (
            "sketch.constraint.tangent",
            vec![
                ("sketch.line", json!({"points":[[10,10],[50,10]],"ids":["a"]})),
                ("sketch.circle.center", json!({"center":[30,20],"radius":8,"id":"b"})),
            ],
            json!({"a":"a","b":"b"}),
        ),
        (
            "sketch.constraint.collinear",
            vec![("sketch.line", json!({"points":[[10,10],[50,10]],"ids":["a"]})), ("sketch.line", json!({"points":[[20,25],[60,30]],"ids":["b"]}))],
            json!({"a":"a","b":"b"}),
        ),
    ];
    for (cmd, geometry, params) in cases {
        let (mut s, drawing, target, _) = sheet_editor();
        for (make, inputs) in geometry {
            run(&mut s, make, inputs);
        }
        let original = s.doc.clone();
        let original_model = s.model.state();
        let before = s.doc.drawing(drawing).unwrap().sheet_sketch(target).unwrap().sketch.clone();
        let (undo, evaluated) = (s.undo.len(), s.evaluations);
        let response = run(&mut s, cmd, params.clone());
        let after = &s.doc.drawing(drawing).unwrap().sheet_sketch(target).unwrap().sketch;
        let constraint_id = response["constraint"].as_str().unwrap();
        assert!(after.constraints.iter().any(|c| c.id == constraint_id), "{cmd}: constraint stored on sheet");
        let direction = |name: &str| {
            let i = after.curve_index(name).unwrap();
            match after.curves[i].kind {
                CurveKind::Line { a, b } => after.points[b].pos - after.points[a].pos,
                _ => panic!("{cmd}: {name} is not a line"),
            }
        };
        let centre = |name: &str| {
            let i = after.curve_index(name).unwrap();
            match after.curves[i].kind {
                CurveKind::Circle { c, .. } => after.points[c].pos,
                _ => panic!("{cmd}: {name} is not a circle"),
            }
        };
        match cmd {
            "sketch.constraint.horizontal_vertical" => assert!(direction("a").y.abs() < 1e-6, "{cmd}: horizontal"),
            "sketch.constraint.equal" => {
                let radius = |name: &str| {
                    let i = after.curve_index(name).unwrap();
                    match after.curves[i].kind {
                        CurveKind::Circle { r, .. } => r,
                        _ => panic!("{cmd}: {name} is not a circle"),
                    }
                };
                assert!((radius("a") - radius("b")).abs() < 1e-6, "{cmd}: equal radii");
            }
            "sketch.constraint.parallel" => assert!(direction("a").cross(direction("b")).abs() < 1e-5, "{cmd}: parallel"),
            "sketch.constraint.perpendicular" => assert!(direction("a").dot(direction("b")).abs() < 1e-5, "{cmd}: perpendicular"),
            "sketch.constraint.concentric" => assert!(centre("a").dist(centre("b")) < 1e-6, "{cmd}: common centre"),
            "sketch.constraint.tangent" => {
                let l = after.curve_index("a").unwrap();
                let CurveKind::Line { a, b } = after.curves[l].kind else { panic!("{cmd}: missing line") };
                let v = after.points[b].pos - after.points[a].pos;
                let circle = after.curve_index("b").unwrap();
                let CurveKind::Circle { c, r } = after.curves[circle].kind else { panic!("{cmd}: missing circle") };
                let centre = after.points[c].pos;
                let gap = (centre - after.points[a].pos).cross(v).abs() / v.dot(v).sqrt();
                assert!((gap - r).abs() < 1e-4, "{cmd}: line-circle tangency");
            }
            "sketch.constraint.collinear" => {
                let a = after.curve_index("a").unwrap();
                let b = after.curve_index("b").unwrap();
                let CurveKind::Line { a: a0, .. } = after.curves[a].kind else { panic!("{cmd}: missing first line") };
                let CurveKind::Line { a: b0, .. } = after.curves[b].kind else { panic!("{cmd}: missing second line") };
                let v = direction("a");
                let w = direction("b");
                let norm = v.dot(v).sqrt();
                assert!(v.cross(w).abs() / (norm * w.dot(w).sqrt()) < 1e-5, "{cmd}: parallel lines");
                assert!((after.points[b0].pos - after.points[a0].pos).cross(v).abs() / norm < 1e-4, "{cmd}: common line");
            }
            "sketch.constraint.midpoint" => {
                let i = after.curve_index("a").unwrap();
                let CurveKind::Line { a, b } = after.curves[i].kind else { panic!("{cmd}: missing line") };
                let centre = (after.points[a].pos + after.points[b].pos) * 0.5;
                let mark = after.points[after.resolve_point("marker").unwrap()].pos;
                assert!(mark.dist(centre) < 1e-6, "{cmd}: marker at line midpoint");
            }
            _ => panic!("unexpected case {cmd}"),
        }
        assert_eq!((&s.doc.features, &s.doc.params), (&original.features, &original.params), "{cmd}: no model writes");
        assert!(Arc::ptr_eq(&original_model, &s.model.state()), "{cmd}: no model evaluation");
        assert_eq!(s.evaluations, evaluated, "{cmd}");
        assert_eq!((s.undo.len(), s.undo.last().unwrap().part), (undo + 1, Some(drawing)), "{cmd}");
        let changed = after.clone();
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(s.doc.drawing(drawing).unwrap().sheet_sketch(target).unwrap().sketch, before, "{cmd}: undo");
        run(&mut s, "edit.redo", json!({}));
        assert_eq!(s.doc.drawing(drawing).unwrap().sheet_sketch(target).unwrap().sketch, changed, "{cmd}: redo");
        let saved = s.doc.to_json();
        let reopened = solidcraft_doc::Document::from_json(&saved).unwrap();
        assert_eq!(reopened.drawing(drawing).unwrap().sheet_sketch(target).unwrap().sketch, changed, "{cmd}: reopen");
    }
}

#[test]
fn sheet_reviewer_constraints_refuse_cross_owner_targets_atomically() {
    let (mut s, d, sk, _) = sheet_editor();
    run(&mut s, "sketch.line", json!({"points":[[20,20],[60,20]],"ids":["a"]}));
    run(&mut s, "sketch.point", json!({"point":[40,30],"id":"marker"}));
    let before = s.doc.clone();
    let entry = s.active_sheet_sketch.clone();
    let counts = (s.undo.len(), s.redo.len(), s.evaluations);
    let model = s.model.state();
    for (cmd, p) in [
        ("sketch.constraint.horizontal_vertical", json!({"sketch":-1,"line":"a"})),
        ("sketch.constraint.equal", json!({"sketch":-1,"a":"a","b":"a"})),
        ("sketch.constraint.parallel", json!({"sketch":-1,"a":"a","b":"a"})),
        ("sketch.constraint.perpendicular", json!({"sketch":-1,"a":"a","b":"a"})),
        ("sketch.constraint.concentric", json!({"sketch":-1,"a":"a","b":"a"})),
        ("sketch.constraint.tangent", json!({"sketch":-1,"a":"a","b":"a"})),
        ("sketch.constraint.collinear", json!({"sketch":-1,"a":"a","b":"a"})),
        ("sketch.constraint.midpoint", json!({"sketch":-1,"point":"marker","line":"a"})),
    ] {
        refused(&mut s, cmd, p);
        assert!(Arc::ptr_eq(&s.doc, &before), "{cmd}");
        assert!(Arc::ptr_eq(&s.model.state(), &model), "{cmd}");
        assert_eq!(s.active_sheet_sketch, entry, "{cmd}");
        assert_eq!((s.undo.len(), s.redo.len(), s.evaluations), counts, "{cmd}");
        assert_eq!(s.doc.drawing(d).unwrap().sheet_sketch(sk).unwrap().sketch.curves.len(), 1);
    }
}

// W38a R10: qualify existing fix/unfix and symmetry through the shared sheet store.
#[test]
fn sheet_reviewer_fix_and_symmetry_keep_model_and_drawing_history_separate() {
    use solidcraft_sketch::ConstraintKind;

    let (mut s, drawing, sketch, _) = sheet_editor();
    run(&mut s, "sketch.point", json!({"point":[30,40],"id":"held"}));
    let original_source = s.doc.clone();
    let model = s.model.state();
    let (undo, evaluated) = (s.undo.len(), s.evaluations);
    let before = s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch.clone();
    let fixed = run(&mut s, "sketch.constraint.fix", json!({"entity":"held","fixed":true}));
    assert_eq!(fixed["result"]["fixed"], true);
    assert_eq!(fixed["result"]["changed"], 1);
    let after = s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch.clone();
    let index = after.resolve_point("held").unwrap();
    assert!(after.constraints.iter().any(|c| matches!(c.kind, ConstraintKind::Fix { p } if p == index)));
    assert_eq!((s.undo.len(), s.undo.last().unwrap().part), (undo + 1, Some(drawing)));
    assert_eq!((&s.doc.features, &s.doc.params), (&original_source.features, &original_source.params));
    assert!(Arc::ptr_eq(&s.model.state(), &model));
    assert_eq!(s.evaluations, evaluated);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch, before);
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch, after);
    let saved = s.doc.to_json();
    let reopened = solidcraft_doc::Document::from_json(&saved).unwrap();
    assert_eq!(reopened.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch, after);
    let released = run(&mut s, "sketch.constraint.fix", json!({"entity":"held","fixed":false}));
    assert_eq!(released["result"]["fixed"], false);
    assert_eq!(released["result"]["changed"], 1);
    let unfixed = &s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch;
    assert!(!unfixed.constraints.iter().any(|c| matches!(c.kind, ConstraintKind::Fix { p } if p == index)));

    let (mut s, drawing, sketch, _) = sheet_editor();
    run(&mut s, "sketch.line", json!({"points":[[50,0],[50,100]],"ids":["axis"]}));
    run(&mut s, "sketch.point", json!({"point":[30,40],"id":"a"}));
    run(&mut s, "sketch.point", json!({"point":[75,43],"id":"b"}));
    let source = s.doc.clone();
    let model = s.model.state();
    let (undo, evaluated) = (s.undo.len(), s.evaluations);
    let before = s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch.clone();
    let result = run(&mut s, "sketch.constraint.symmetry", json!({"a":"a","b":"b","line":"axis"}));
    assert_eq!(result["constraints"].as_array().unwrap().len(), 1);
    let after = s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch.clone();
    let pa = after.point(after.resolve_point("a").unwrap()).unwrap();
    let pb = after.point(after.resolve_point("b").unwrap()).unwrap();
    let axis = after.curve_index("axis").unwrap();
    let solidcraft_sketch::CurveKind::Line { a, b } = after.curves[axis].kind else { panic!("missing symmetry axis") };
    let p0 = after.point(a).unwrap();
    let direction = after.point(b).unwrap() - p0;
    let length = direction.len();
    assert!(length > 1.0);
    assert!((((pa + pb) * 0.5 - p0).cross(direction) / length).abs() < 1e-4);
    assert!(((pb - pa).dot(direction) / length).abs() < 1e-4);
    assert!(after.constraints.iter().any(|c| matches!(c.kind, ConstraintKind::Symmetric { .. })));
    assert_eq!((s.undo.len(), s.undo.last().unwrap().part), (undo + 1, Some(drawing)));
    assert_eq!((&s.doc.features, &s.doc.params), (&source.features, &source.params));
    assert!(Arc::ptr_eq(&s.model.state(), &model));
    assert_eq!(s.evaluations, evaluated);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch, before);
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch, after);
    let saved = s.doc.to_json();
    let reopened = solidcraft_doc::Document::from_json(&saved).unwrap();
    assert_eq!(reopened.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch, after);
}

#[test]
fn sheet_reviewer_fix_and_symmetry_refuse_foreign_targets_atomically() {
    let (mut s, drawing, sketch, _) = sheet_editor();
    run(&mut s, "sketch.line", json!({"points":[[50,0],[50,100]],"ids":["axis"]}));
    run(&mut s, "sketch.point", json!({"point":[30,40],"id":"a"}));
    run(&mut s, "sketch.point", json!({"point":[75,43],"id":"b"}));
    let before = s.doc.clone();
    let model = s.model.state();
    let entry = s.active_sheet_sketch.clone();
    let history = (s.undo.len(), s.redo.len(), s.evaluations);
    for (command, args) in [
        ("sketch.constraint.fix", json!({"sketch":-1,"entity":"a","fixed":true})),
        ("sketch.constraint.symmetry", json!({"sketch":-1,"a":"a","b":"b","line":"axis"})),
        ("sketch.constraint.fix", json!({"entity":"foreign","fixed":true})),
        ("sketch.constraint.symmetry", json!({"a":"a","b":"b","line":"foreign"})),
    ] {
        refused(&mut s, command, args);
        assert!(Arc::ptr_eq(&s.doc, &before), "{command}");
        assert!(Arc::ptr_eq(&s.model.state(), &model), "{command}");
        assert_eq!(s.active_sheet_sketch, entry, "{command}");
        assert_eq!((s.undo.len(), s.redo.len(), s.evaluations), history, "{command}");
        assert_eq!(s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch.curves.len(), 1);
    }
}

// W38a R11: construction and deletion are editing operations on the shared sheet sketch.
#[test]
fn sheet_reviewer_construction_toggle_is_one_drawing_edit() {
    let (mut s, drawing, sketch, _) = sheet_editor();
    run(&mut s, "sketch.line", json!({"points":[[10,20],[50,20]],"ids":["guide"]}));
    let before = s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch.clone();
    let source = s.doc.clone();
    let model = s.model.state();
    let (steps, evaluations) = (s.undo.len(), s.evaluations);

    let changed = run(&mut s, "sketch.construction", json!({"curves":["guide"],"value":true}));
    assert_eq!((changed["changed"].as_u64(), changed["construction"].as_bool()), (Some(1), Some(true)));
    let after = s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch.clone();
    assert!(after.curves[after.curve_index("guide").unwrap()].construction);
    assert_eq!((&s.doc.features, &s.doc.params), (&source.features, &source.params));
    assert!(Arc::ptr_eq(&s.model.state(), &model));
    assert_eq!(s.evaluations, evaluations);
    assert_eq!((s.undo.len(), s.undo.last().unwrap().part), (steps + 1, Some(drawing)));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch, before);
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch, after);
    assert_eq!(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap().drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch, after);
    let back = run(&mut s, "sketch.construction", json!({"curves":["guide"],"value":false}));
    assert_eq!((back["changed"].as_u64(), back["construction"].as_bool()), (Some(1), Some(false)));
}

#[test]
fn sheet_reviewer_delete_keeps_source_and_refuses_foreign_or_missing_entities() {
    let (mut s, drawing, sketch, _) = sheet_editor();
    run(&mut s, "sketch.line", json!({"points":[[10,20],[30,20]],"ids":["remove_me"]}));
    run(&mut s, "sketch.line", json!({"points":[[40,50],[60,50]],"ids":["keep_me"]}));
    let source = s.doc.clone();
    let model = s.model.state();
    let (steps, evaluations) = (s.undo.len(), s.evaluations);
    let before = s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch.clone();
    assert!(before.curve_index("remove_me").is_some() && before.curve_index("keep_me").is_some());

    let result = run(&mut s, "sketch.delete", json!({"entities":["remove_me"]}));
    assert_eq!(result["deleted"], 1);
    let after = s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch.clone();
    assert!(after.curve_index("remove_me").is_none() && after.curve_index("keep_me").is_some());
    assert_eq!((&s.doc.features, &s.doc.params), (&source.features, &source.params));
    assert!(Arc::ptr_eq(&s.model.state(), &model));
    assert_eq!(s.evaluations, evaluations);
    assert_eq!((s.undo.len(), s.undo.last().unwrap().part), (steps + 1, Some(drawing)));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch, before);
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch, after);
    assert_eq!(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap().drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch, after);

    let unchanged = s.doc.clone();
    let entry = s.active_sheet_sketch.clone();
    let history = (s.undo.len(), s.redo.len(), s.evaluations);
    for (command, args) in [
        ("sketch.construction", json!({"sketch":-1,"curves":["keep_me"]})),
        ("sketch.delete", json!({"sketch":-1,"entities":["keep_me"]})),
        ("sketch.construction", json!({"curves":["not_here"]})),
        ("sketch.delete", json!({"entities":["not_here"]})),
    ] {
        refused(&mut s, command, args);
        assert!(Arc::ptr_eq(&s.doc, &unchanged), "{command}");
        assert!(Arc::ptr_eq(&s.model.state(), &model), "{command}");
        assert_eq!(s.active_sheet_sketch, entry, "{command}");
        assert_eq!((s.undo.len(), s.redo.len(), s.evaluations), history, "{command}");
    }
}

// W38a R12: drawing-local reference dimensions do not define source parameters.
#[test]
fn sheet_reviewer_reference_dimension_measures_paper_mm_and_tracks_drawing_undo() {
    let (mut s, drawing, sketch, _) = sheet_editor();
    run(&mut s, "sketch.line", json!({"points": [[10, 20], [50, 20]], "ids": ["measured_line"]}));
    let source = s.doc.clone();
    let model = s.model.state();
    let (steps, evaluations) = (s.undo.len(), s.evaluations);

    let measurement = run(&mut s, "sketch.dimension", json!({"entities": ["measured_line"], "driven": true}));
    assert_eq!(measurement["param"], Value::Null);
    assert_eq!(measurement["driven"], true);
    assert_eq!(measurement["unit"], "mm", "paper lengths must not follow inch model display units");
    assert!((measurement["measured"].as_f64().unwrap() - 40.0).abs() < 1e-7, "{measurement}");
    let after_dimension = s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch.clone();
    let dimension = after_dimension.constraints.last().unwrap();
    assert!(dimension.driven && dimension.param.is_none());
    let dimension_id = dimension.id.clone();
    assert_eq!((&s.doc.params, &s.doc.features), (&source.params, &source.features));
    assert!(Arc::ptr_eq(&s.model.state(), &model));
    assert_eq!(s.evaluations, evaluations);
    assert_eq!((s.undo.len(), s.undo.last().unwrap().part), (steps + 1, Some(drawing)));

    let placed = run(&mut s, "sketch.dimension_text", json!({"dimension": dimension_id, "at": [30, 32]}));
    assert_eq!(placed["dimension"], dimension_id);
    let with_text = s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch.clone();
    assert!(with_text.constraints.last().unwrap().text.is_some());
    assert_eq!((s.undo.len(), s.undo.last().unwrap().part), (steps + 2, Some(drawing)));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch, after_dimension);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch.constraints.len(), after_dimension.constraints.len() - 1);
    run(&mut s, "edit.redo", json!({}));
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch, with_text);
    let reopened = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
    assert_eq!(reopened.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch, with_text);
    assert_eq!((&s.doc.params, &s.doc.features), (&source.params, &source.features));
    assert!(Arc::ptr_eq(&s.model.state(), &model));
    assert_eq!(s.evaluations, evaluations);
}

#[test]
fn sheet_reviewer_reference_dimensions_refuse_invalid_values_and_wrong_owners_atomically() {
    let (mut s, drawing, sketch, _) = sheet_editor();
    run(&mut s, "sketch.line", json!({"points": [[10, 20], [50, 20]], "ids": ["measured_line"]}));
    let before = s.doc.clone();
    let model = s.model.state();
    let entry = s.active_sheet_sketch.clone();
    let history = (s.undo.len(), s.redo.len(), s.evaluations);

    for (command, args) in [
        ("sketch.dimension", json!({"entities": ["measured_line"], "value": "Width = 20"})),
        ("sketch.dimension", json!({"entities": ["measured_line"], "driven": true, "value": "20 mm"})),
        ("sketch.dimension", json!({"entities": ["measured_line"], "driven": true, "scale_sketch": true})),
        ("sketch.dimension", json!({"entities": ["measured_line"], "driven": true, "sketch": -1})),
        ("sketch.dimension", json!({"entities": ["unknown"], "driven": true})),
        ("sketch.dimension_text", json!({"dimension": "missing", "at": [0, 0]})),
        ("sketch.dimension_text", json!({"sketch": -1, "dimension": "missing", "at": [0, 0]})),
    ] {
        refused(&mut s, command, args.clone());
        assert!(Arc::ptr_eq(&s.doc, &before), "{command} {args}");
        assert!(Arc::ptr_eq(&s.model.state(), &model), "{command} {args}");
        assert_eq!(s.active_sheet_sketch, entry, "{command} {args}");
        assert_eq!((s.undo.len(), s.redo.len(), s.evaluations), history, "{command} {args}");
        assert_eq!(s.open_drawing, Some(drawing));
        assert!(s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch.constraints.is_empty());
    }
}

// W38a R13: the existing Coincident command reads and edits the exact sheet sketch.
#[test]
fn sheet_reviewer_coincident_points_are_owned_idempotent_and_undoable() {
    let (mut s, drawing, sketch, _) = sheet_editor();
    run(&mut s, "sketch.point", json!({"point": [15, 20], "id": "pin_a", "connect": false}));
    run(&mut s, "sketch.point", json!({"point": [70, 45], "id": "pin_b", "connect": false}));
    let source = s.doc.clone();
    let model = s.model.state();
    let evaluations = s.evaluations;
    let steps = s.undo.len();
    let before = s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch.clone();

    let added = run(&mut s, "sketch.constraint.coincident", json!({"a": "pin_a", "b": "pin_b"}));
    let after = s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch.clone();
    let a = after.resolve_point("pin_a").unwrap();
    let b = after.resolve_point("pin_b").unwrap();
    assert!(after.point(a).unwrap().dist(after.point(b).unwrap()) < 1e-7, "{added}");
    assert_eq!(after.constraints.len(), before.constraints.len() + 1);
    assert_eq!((s.undo.len(), s.undo.last().unwrap().part), (steps + 1, Some(drawing)));
    let published = s.doc.clone();

    let again = run(&mut s, "sketch.constraint.coincident", json!({"a": "pin_b", "b": "pin_a"}));
    assert_eq!(again["existing"], true);
    assert_eq!(again["constraint"], added["constraint"]);
    assert!(Arc::ptr_eq(&s.doc, &published), "an existing constraint is a read-only result");
    assert_eq!(s.undo.len(), steps + 1);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch, before);
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch, after);
    let reopened = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
    assert_eq!(reopened.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch, after);
    assert_eq!((&s.doc.features, &s.doc.params), (&source.features, &source.params));
    assert!(Arc::ptr_eq(&s.model.state(), &model));
    assert_eq!(s.evaluations, evaluations);
}

#[test]
fn sheet_reviewer_coincident_curve_preflight_refuses_foreign_targets_atomically() {
    let (mut s, drawing, sketch, _) = sheet_editor();
    run(&mut s, "sketch.line", json!({"points": [[10, 20], [50, 20]], "ids": ["wall"], "connect": false}));
    run(&mut s, "sketch.point", json!({"point": [35, 40], "id": "peg", "connect": false}));
    let before = s.doc.clone();
    let model = s.model.state();
    let entry = s.active_sheet_sketch.clone();
    let history = (s.undo.len(), s.redo.len(), s.evaluations);
    for params in [
        json!({"sketch": -1, "a": "peg", "b": "wall"}),
        json!({"sketch": ids::encode(drawing), "a": "peg", "b": "wall"}),
        json!({"a": "not_here", "b": "wall"}),
        json!({"a": "peg", "b": "missing_curve"}),
        json!({"a": "wall", "b": "wall"}),
    ] {
        refused(&mut s, "sketch.constraint.coincident", params.clone());
        assert!(Arc::ptr_eq(&s.doc, &before), "{params}");
        assert!(Arc::ptr_eq(&s.model.state(), &model), "{params}");
        assert_eq!(s.active_sheet_sketch, entry, "{params}");
        assert_eq!((s.undo.len(), s.redo.len(), s.evaluations), history, "{params}");
    }
    let applied = run(&mut s, "sketch.constraint.coincident", json!({"a": "wall", "b": "peg"}));
    let after = s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch.clone();
    assert_eq!(applied["constraint"], after.constraints.last().unwrap().id);
    assert!(matches!(&after.constraints.last().unwrap().kind, solidcraft_sketch::ConstraintKind::PointOnCurve { .. }));
    assert_eq!(s.undo.last().unwrap().part, Some(drawing));
    assert_eq!((&s.doc.features, &s.doc.params), (&before.features, &before.params));
    assert!(Arc::ptr_eq(&s.model.state(), &model));
    assert_eq!(s.evaluations, history.2);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch.constraints.len(), 0);
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch, after);
}

#[test]
fn sheet_reviewer_coincident_lines_join_only_their_local_endpoints() {
    let (mut s, drawing, sketch, _) = sheet_editor();
    run(&mut s, "sketch.line", json!({"points": [[20, 20], [60, 20]], "ids": ["first"], "connect": false}));
    run(&mut s, "sketch.line", json!({"points": [[70, 25], [70, 55]], "ids": ["second"], "connect": false}));
    let before = s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch.clone();
    let model = s.model.state();
    let source = s.doc.clone();
    let steps = s.undo.len();
    let applied = run(&mut s, "sketch.constraint.coincident", json!({"a": "first", "b": "second"}));
    let after = s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch.clone();
    let matched = after.constraints.last().unwrap();
    assert_eq!(applied["constraint"], matched.id);
    let solidcraft_sketch::ConstraintKind::Coincident { p, q } = &matched.kind else {
        panic!("joining two lines must join their nearest endpoints");
    };
    assert!(after.point(*p).unwrap().dist(after.point(*q).unwrap()) < 1e-7);
    assert_eq!((s.undo.len(), s.undo.last().unwrap().part), (steps + 1, Some(drawing)));
    assert_eq!((&s.doc.features, &s.doc.params), (&source.features, &source.params));
    assert!(Arc::ptr_eq(&s.model.state(), &model));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch, before);
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch, after);
}

/// R14: read-only curve selection and solve reports share the exact sheet receiver.
#[test]
fn sheet_reviewer_chain_and_solve_are_paper_owned_read_only_queries() {
    let (mut s, drawing, sketch, made) = sheet_editor();
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [15, 10], "p1": [55, 30]}));
    let before = s.doc.clone();
    let model = s.model.state();
    let entry = s.active_sheet_sketch.clone();
    let history = (s.undo.len(), s.redo.len(), s.evaluations);
    let inspected = run(&mut s, "drawing.sketch.inspect", json!({}));

    let chain = run(&mut s, "sketch.chain", json!({"sketch": made["sketch"], "curve": "l1"}));
    let ids = chain["curves"].as_array().unwrap();
    assert_eq!(ids.len(), 4, "the rectangle is one sheet-local connected chain: {chain}");
    assert!(ids.iter().any(|id| id == "l1"));
    let solved = run(&mut s, "sketch.solve", json!({"sketch": made["sketch"]}));
    assert_eq!(solved["profiles"], 1, "the existing profile finder sees one paper rectangle: {solved}");
    assert_eq!(solved["dof"], inspected["dof"], "inspection and solve use the same paper sketch");
    assert!(!solved["status"].is_null(), "the actual solver status must be reported");

    assert!(Arc::ptr_eq(&s.doc, &before), "neither read-only command publishes a drawing");
    assert!(Arc::ptr_eq(&s.model.state(), &model), "neither query evaluates the model");
    assert_eq!(s.active_sheet_sketch, entry);
    assert_eq!((s.undo.len(), s.redo.len(), s.evaluations), history);
    assert_eq!(s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch.curves.len(), 4);
}

/// R14: no implicit switch to a similarly named model sketch or another sheet.
#[test]
fn sheet_reviewer_chain_and_solve_refuse_foreign_or_missing_targets_atomically() {
    let (mut s, drawing, sketch, _) = sheet_editor();
    run(&mut s, "sketch.line", json!({"points": [[10, 10], [30, 20]], "ids": ["only_sheet_line"]}));
    let before = s.doc.clone();
    let model = s.model.state();
    let entry = s.active_sheet_sketch.clone();
    let history = (s.undo.len(), s.redo.len(), s.evaluations);
    for (command, params) in [
        ("sketch.chain", json!({"sketch": ids::encode(drawing), "curve": "only_sheet_line"})),
        ("sketch.chain", json!({"sketch": "not-an-id", "curve": "only_sheet_line"})),
        ("sketch.chain", json!({"curve": "no_such_curve"})),
        ("sketch.solve", json!({"sketch": ids::encode(drawing)})),
        ("sketch.solve", json!({"sketch": -1})),
        ("sketch.solve", json!({"sketch": true})),
    ] {
        refused(&mut s, command, params.clone());
        assert!(Arc::ptr_eq(&s.doc, &before), "{command}: {params}");
        assert!(Arc::ptr_eq(&s.model.state(), &model), "{command}: {params}");
        assert_eq!(s.active_sheet_sketch, entry, "{command}: {params}");
        assert_eq!((s.undo.len(), s.redo.len(), s.evaluations), history, "{command}: {params}");
    }
    assert_eq!(run(&mut s, "sketch.chain", json!({"curve": "only_sheet_line"}))["curves"], json!(["only_sheet_line"]));
    assert_eq!(run(&mut s, "sketch.solve", json!({}))["profiles"], 0);
    assert_eq!(s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch.curves.len(), 1);
    assert!(Arc::ptr_eq(&s.doc, &before));
}
/// R15: palette state uses the exact paper sketch and drawing-local history.
#[test]
fn sheet_reviewer_palette_options_are_drawing_local_and_undoable() {
    let (mut s, drawing, sketch, made) = sheet_editor();
    let snapshot = s.doc.clone();
    let model = s.model.state();
    let evaluations = s.evaluations;
    let history = s.undo.len();
    let entry = s.active_sheet_sketch.clone();
    let current = run(&mut s, "sketch.options", json!({"sketch": made["sketch"]}));
    assert!(current["options"]["grid"].is_boolean(), "{current}");
    assert!(current["options"]["show_points"].is_boolean(), "{current}");
    assert!(Arc::ptr_eq(&s.doc, &snapshot), "reading palette options is non-mutating");
    assert_eq!(s.undo.len(), history);
    assert_eq!(s.active_sheet_sketch, entry);

    let updated = run(&mut s, "sketch.options", json!({"sketch": made["sketch"], "grid": false, "show_points": false, "snap": false}));
    assert_eq!(updated["options"]["grid"], false);
    assert_eq!(updated["options"]["show_points"], false);
    assert_eq!(updated["options"]["snap"], false);
    let kept = s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch.clone();
    assert!(kept.view.hide_grid && kept.view.hide_points && kept.view.no_snap);
    assert_eq!((s.undo.len(), s.undo.last().unwrap().part), (history + 1, Some(drawing)));
    assert_eq!(s.active_sheet_sketch, entry);
    assert_eq!((&s.doc.features, &s.doc.params), (&snapshot.features, &snapshot.params));
    assert!(Arc::ptr_eq(&s.model.state(), &model));
    assert_eq!(s.evaluations, evaluations);

    run(&mut s, "edit.undo", json!({}));
    assert_eq!(
        s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch,
        snapshot.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch
    );
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch, kept);
    assert_eq!(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap().drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch, kept);
}

/// R15: the existing point drag places paper geometry even under inch display units.
#[test]
fn sheet_reviewer_point_move_is_paper_mm_one_drawing_step_and_reopens() {
    let (mut s, drawing, sketch, made) = sheet_editor();
    run(&mut s, "sketch.point", json!({"point": [15, 20], "id": "free_pin", "connect": false}));
    let previous = s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch.clone();
    let source = s.doc.clone();
    let model = s.model.state();
    let evaluations = s.evaluations;
    let history = s.undo.len();
    let moved = run(&mut s, "sketch.move_point", json!({"sketch": made["sketch"], "point": "free_pin", "to": [25, 35]}));
    assert!(moved["sketch"]["dof"].is_number(), "{moved}");
    let after = s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch.clone();
    let point = after.resolve_point("free_pin").unwrap();
    let actual = after.point(point).unwrap();
    assert!((actual.x - 25.0).abs() < 1e-6 && (actual.y - 35.0).abs() < 1e-6, "{actual:?}");
    assert_eq!((s.undo.len(), s.undo.last().unwrap().part), (history + 1, Some(drawing)));
    assert_eq!((&s.doc.features, &s.doc.params), (&source.features, &source.params));
    assert!(Arc::ptr_eq(&s.model.state(), &model));
    assert_eq!(s.evaluations, evaluations);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch, previous);
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch, after);
    let reopened = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
    assert_eq!(reopened.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch, after);
}

#[test]
fn sheet_reviewer_palette_and_point_move_refuse_model_only_or_wrong_owner_atomically() {
    let (mut s, drawing, sketch, made) = sheet_editor();
    run(&mut s, "sketch.point", json!({"point": [15, 20], "id": "free_pin", "connect": false}));
    let before = s.doc.clone();
    let model = s.model.state();
    let entry = s.active_sheet_sketch.clone();
    let history = (s.undo.len(), s.redo.len(), s.evaluations);
    for (command, params) in [
        ("sketch.options", json!({"slice": true})),
        ("sketch.options", json!({"sketch_3d": true})),
        ("sketch.options", json!({"sketch": ids::encode(drawing), "grid": false})),
        ("sketch.options", json!({"sketch": 18446744073709551615u64})),
        ("sketch.move_point", json!({"point": "absent", "to": [30, 40]})),
        ("sketch.move_point", json!({"sketch": ids::encode(drawing), "point": "free_pin", "to": [30, 40]})),
        ("sketch.move_point", json!({"sketch": made["sketch"], "point": "free_pin", "to": ["1 mm", 40]})),
    ] {
        refused(&mut s, command, params.clone());
        assert!(Arc::ptr_eq(&s.doc, &before), "{command}: {params}");
        assert!(Arc::ptr_eq(&s.model.state(), &model), "{command}: {params}");
        assert_eq!(s.active_sheet_sketch, entry, "{command}: {params}");
        assert_eq!((s.undo.len(), s.redo.len(), s.evaluations), history, "{command}: {params}");
        assert_eq!(s.open_drawing, Some(drawing));
        assert!(s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).is_some());
    }
}

/// R16: driving dimensions are part of a drawing, never newly created model parameters.
#[test]
fn sheet_reviewer_driving_dimension_uses_paper_mm_and_one_drawing_step() {
    let (mut s, drawing, sketch, made) = sheet_editor();
    run(&mut s, "sketch.line", json!({"points": [[10, 20], [50, 20]], "ids": ["sized_line"]}));
    let before = s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch.clone();
    let source = s.doc.clone();
    let model = s.model.state();
    let (steps, evals) = (s.undo.len(), s.evaluations);

    let result = run(&mut s, "sketch.dimension", json!({"sketch": made["sketch"], "entities": ["sized_line"], "value": 50}));
    assert_eq!(result["expression"], "50");
    assert_eq!(result["param"], Value::Null, "the source model has no sheet dimension parameter");
    assert_eq!(result["driven"], false);
    assert_eq!(result["unit"], "mm", "an inch-design bare length is still paper millimetres");
    assert!((result["measured"].as_f64().unwrap() - 50.0).abs() < 1e-5, "{result}");

    let after = s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch.clone();
    let constraint = after.constraints.last().unwrap();
    assert!(!constraint.driven);
    assert_eq!(constraint.param.as_deref(), Some("50"));
    assert!((solidcraft_sketch::measure_dimension(&after, &constraint.kind).unwrap() - 50.0).abs() < 1e-5);
    assert_eq!((s.undo.len(), s.undo.last().unwrap().part), (steps + 1, Some(drawing)));
    assert_eq!((&s.doc.features, &s.doc.params), (&source.features, &source.params));
    assert!(Arc::ptr_eq(&s.model.state(), &model));
    assert_eq!(s.evaluations, evals);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch, before);
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch, after);
    let reopened = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
    assert_eq!(reopened.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch, after);
    assert_eq!(s.evaluations, evals);
}

/// R16: stored expressions are resolved against source parameters, but live in the drawing.
#[test]
fn sheet_reviewer_driving_expression_reopens_and_recomputes_without_model_edits() {
    let (mut s, drawing, sketch, _) = sheet_editor();
    s.doc_mut().set_param("Inset", "1", Some("in"), None).unwrap();
    run(&mut s, "sketch.line", json!({"points": [[10, 20], [50, 20]], "ids": ["source_line"]}));
    let source = s.doc.clone();
    let model = s.model.state();
    let evals = s.evaluations;
    let authored = "Inset + 5";
    let result = run(&mut s, "sketch.dimension", json!({"entities": ["source_line"], "value": authored}));
    assert_eq!(result["expression"], authored);
    assert_eq!(result["param"], Value::Null);
    assert_eq!(result["unit"], "mm");
    assert!((result["measured"].as_f64().unwrap() - 30.4).abs() < 1e-5, "{result}");
    let kept = s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch.clone();
    assert_eq!(kept.constraints.last().unwrap().param.as_deref(), Some(authored));
    assert_eq!((&s.doc.params, &s.doc.features), (&source.params, &source.features));
    assert!(Arc::ptr_eq(&s.model.state(), &model));
    assert_eq!(s.evaluations, evals);

    let reopened = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
    let solved = reopened.solve_sheet_sketch(drawing, sketch).unwrap();
    let c = solved.sketch.constraints.last().unwrap();
    assert!((solidcraft_sketch::measure_dimension(&solved.sketch, &c.kind).unwrap() - 30.4).abs() < 1e-5);
    let mut changed_source = reopened.clone();
    changed_source.set_param("Inset", "2", Some("in"), None).unwrap();
    let solved_again = changed_source.solve_sheet_sketch(drawing, sketch).unwrap();
    let c = solved_again.sketch.constraints.last().unwrap();
    assert!((solidcraft_sketch::measure_dimension(&solved_again.sketch, &c.kind).unwrap() - 55.8).abs() < 1e-4);
    assert_eq!(changed_source.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch, kept, "solving is read-only");
}

/// R16: errors, foreign identities, typed source values and definitions must be atomic.
#[test]
fn sheet_reviewer_driving_dimension_rejects_invalid_inputs_without_publication() {
    let (mut s, drawing, sketch, made) = sheet_editor();
    s.doc_mut().set_param("Angle", "90", Some("deg"), None).unwrap();
    run(&mut s, "sketch.line", json!({"points": [[10, 20], [50, 20]], "ids": ["sized_line"]}));
    let source = s.doc.clone();
    let model = s.model.state();
    let entry = s.active_sheet_sketch.clone();
    let history = (s.undo.len(), s.redo.len(), s.evaluations);
    for args in [
        json!({"entities": ["sized_line"], "value": "Width = 20"}),
        json!({"entities": ["sized_line"], "value": "Angle"}),
        json!({"entities": ["sized_line"], "value": "Missing + 2"}),
        json!({"entities": ["sized_line"], "value": "1 / 0"}),
        json!({"entities": ["sized_line"], "value": "8 cm", "scale_sketch": true}),
        json!({"entities": ["sized_line"], "value": "25", "if_redundant": "driven"}),
        json!({"entities": ["sized_line"], "value": "1".repeat(4097)}),
        json!({"entities": ["sized_line"], "value": true}),
        json!({"entities": ["sized_line"], "value": null}),
        json!({"entities": ["sized_line"], "value": "  "}),
        json!({"entities": ["sized_line"], "value": ["50"]}),
        json!({"entities": ["missing"], "value": 50}),
        json!({"sketch": ids::encode(drawing), "entities": ["sized_line"], "value": 50}),
        json!({"sketch": -1, "entities": ["sized_line"], "value": 50}),
        json!({"sketch": made["drawing"], "entities": ["sized_line"], "value": 50}),
    ] {
        refused(&mut s, "sketch.dimension", args.clone());
        assert!(Arc::ptr_eq(&s.doc, &source), "changed source on {args}");
        assert!(Arc::ptr_eq(&s.model.state(), &model), "evaluated source on {args}");
        assert_eq!(s.active_sheet_sketch, entry, "{args}");
        assert_eq!((s.undo.len(), s.redo.len(), s.evaluations), history, "{args}");
        assert!(s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch.constraints.is_empty());
    }
}

/// W38a R17: the existing 2D edit commands are paper-owned transactions, not model edits.
#[test]
fn sheet_batch_move_scale_break_trim_and_drag_publish_only_the_selected_drawing() {
    for command in ["sketch.move", "sketch.scale", "sketch.break", "sketch.trim", "sketch.drag"] {
        let (mut s, drawing, sketch, made) = sheet_editor();
        let line = run(
            &mut s,
            "sketch.line",
            json!({
                "points": [[15, 20], [55, 20]], "connect": false, "infer": false
            }),
        )["curves"][0]
            .as_str()
            .unwrap()
            .to_string();
        let params = match command {
            "sketch.move" => json!({"entities": [line], "translate": [8, 3]}),
            "sketch.scale" => json!({"entities": [line], "base": [0, 0], "factor": 1.5}),
            "sketch.break" => json!({"curve": line, "points": [[35, 20]]}),
            "sketch.trim" => json!({"curve": line, "at": [35, 20]}),
            "sketch.drag" => json!({"entity": line, "from": [35, 20], "to": [42, 26]}),
            _ => {
                assert!(command.is_empty(), "unexpected 2D test command: {command}");
                Value::Null
            }
        };
        let before = s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch.clone();
        let source = s.doc.clone();
        let model = s.model.state();
        let history = s.undo.len();
        let evals = s.evaluations;
        let result = run(&mut s, command, params);
        assert!(result["sketch"].is_object(), "{command}: {result}");
        let after = s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch.clone();
        assert_ne!(after, before, "{command} must modify the selected sheet");
        assert_eq!((s.undo.len(), s.undo.last().unwrap().part), (history + 1, Some(drawing)), "{command}");
        assert_eq!((&s.doc.features, &s.doc.params), (&source.features, &source.params), "{command}");
        assert!(Arc::ptr_eq(&model, &s.model.state()), "{command}");
        assert_eq!(s.evaluations, evals, "{command}");
        assert_eq!(s.active_sheet_sketch.as_ref().unwrap().sketch, sketch, "{command}");
        assert_eq!(run(&mut s, "drawing.sketch.inspect", json!({}))["sketch"], made["sketch"]);
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch, before, "{command}");
        run(&mut s, "edit.redo", json!({}));
        assert_eq!(s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch, after, "{command}");
        let reopened = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
        assert_eq!(reopened.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch, after, "{command}");
    }
}

/// One modify command that looks at the other curves must not borrow a model sketch.
#[test]
fn sheet_batch_extend_and_path_trim_keep_paper_geometry_without_model_pruning() {
    let (mut s, drawing, sketch, _) = sheet_editor();
    s.doc_mut().new_model_param("10 mm", "mm");
    run(&mut s, "sketch.line", json!({"points": [[30, 5], [30, 45]], "connect": false, "infer": false}));
    let extending = run(&mut s, "sketch.line", json!({"points": [[5, 25], [15, 25]], "connect": false, "infer": false}))["curves"][0]
        .as_str()
        .unwrap()
        .to_string();
    let snapshot = s.doc.clone();
    let model = s.model.state();
    let before_steps = s.undo.len();
    let extended = run(&mut s, "sketch.extend", json!({"curve": extending, "at": [15, 25]}));
    assert!(extended["sketch"].is_object());
    let updated = &s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch;
    let end = updated.point(updated.resolve_point(&format!("{extending}.end")).unwrap()).unwrap();
    assert!((end.x - 30.0).abs() < 1e-6 && (end.y - 25.0).abs() < 1e-6, "{end:?}");
    assert_eq!((&s.doc.params, &s.doc.features), (&snapshot.params, &snapshot.features));
    assert!(Arc::ptr_eq(&model, &s.model.state()));
    assert_eq!((s.undo.len(), s.undo.last().unwrap().part), (before_steps + 1, Some(drawing)));

    let before_trim = s.doc.clone();
    let history = s.undo.len();
    let trimmed = run(&mut s, "sketch.trim.path", json!({"points": [[20, 15], [40, 15]]}));
    assert!(trimmed["trimmed"].as_array().is_some_and(|a| !a.is_empty()), "{trimmed}");
    assert_eq!((&s.doc.params, &s.doc.features), (&snapshot.params, &snapshot.features));
    assert_eq!((s.undo.len(), s.undo.last().unwrap().part), (history + 1, Some(drawing)));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(
        s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch,
        before_trim.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch
    );
}

/// Existing fillet/chamfer geometry must use an authored sheet expression, never a model param.
#[test]
fn sheet_batch_fillet_chamfer_and_rounded_rectangle_are_drawing_local() {
    for command in
        ["sketch.fillet", "sketch.chamfer.equal_distance", "sketch.chamfer.two_distance", "sketch.chamfer.distance_angle", "sketch.rectangle.rounded"]
    {
        let (mut s, drawing, sketch, _) = sheet_editor();
        s.doc_mut().set_param("Inset", "1", Some("in"), None).unwrap();
        let before = s.doc.clone();
        let model = s.model.state();
        let evals = s.evaluations;
        let result = if command == "sketch.rectangle.rounded" {
            run(&mut s, command, json!({"p0": [20, 20], "p1": [90, 60], "radius": "Inset / 5"}))
        } else {
            let sides = run(&mut s, "sketch.rectangle.two_point", json!({"p0": [20, 20], "p1": [90, 60]}))["curves"].as_array().unwrap().clone();
            let mut request = json!({"a": sides[0], "b": sides[1]});
            match command {
                "sketch.fillet" => request["radius"] = json!("Inset / 5"),
                "sketch.chamfer.equal_distance" => request["distance"] = json!(4),
                "sketch.chamfer.two_distance" => {
                    request["distance"] = json!(3);
                    request["distance2"] = json!(4);
                }
                "sketch.chamfer.distance_angle" => {
                    request["distance"] = json!(4);
                    request["angle"] = json!(45);
                }
                _ => assert!(command.is_empty(), "unexpected corner test command: {command}"),
            };
            run(&mut s, command, request)
        };
        assert!(result["curves"].is_array(), "{command}: {result}");
        let saved = &s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch;
        assert!(saved.curves.len() >= 5, "{command}");
        if command == "sketch.fillet" || command == "sketch.rectangle.rounded" {
            assert_eq!(result["param"], Value::Null, "{command}: no model parameter");
            assert_eq!(result["expression"], "Inset / 5", "{command}");
            assert!(saved.constraints.iter().any(|c| c.param.as_deref() == Some("Inset / 5")), "{command}");
        }
        assert_eq!((&s.doc.features, &s.doc.params), (&before.features, &before.params), "{command}");
        assert!(Arc::ptr_eq(&s.model.state(), &model), "{command}");
        assert_eq!(s.evaluations, evals, "{command}");
        let reopened = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
        assert_eq!(reopened.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch, *saved, "{command}");
    }
}

/// A compound creator must stage its creator and dimensions atomically under the same sheet.
#[test]
fn sheet_batch_dimensioned_creation_respects_paper_units_and_owner() {
    let (mut s, drawing, sketch, made) = sheet_editor();
    s.doc_mut().set_param("Inset", "1", Some("in"), None).unwrap();
    let source = s.doc.clone();
    let model = s.model.state();
    let history = s.undo.len();
    let evals = s.evaluations;
    let out = run(
        &mut s,
        "sketch.draw.dimensioned",
        json!({
            "command": "sketch.line", "params": {"points": [[10, 15], [50, 15]], "connect": false},
            "dimensions": {"length": "Inset + 5"}
        }),
    );
    assert_eq!(out["dimensions"]["length"]["param"], Value::Null);
    assert_eq!(out["dimensions"]["length"]["expression"], "Inset + 5");
    assert_eq!(out["dimensions"]["length"]["unit"], "mm");
    let record = &s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch;
    assert!(record.constraints.iter().any(|c| c.param.as_deref() == Some("Inset + 5")));
    let dimension = record.constraints.iter().find(|c| c.param.as_deref() == Some("Inset + 5")).unwrap();
    assert!((solidcraft_sketch::measure_dimension(record, &dimension.kind).unwrap() - 30.4).abs() < 1e-5);
    assert_eq!((s.undo.len(), s.undo.last().unwrap().part), (history + 1, Some(drawing)));
    assert_eq!((&s.doc.features, &s.doc.params), (&source.features, &source.params));
    assert!(Arc::ptr_eq(&s.model.state(), &model));
    assert_eq!(s.evaluations, evals);
    run(&mut s, "edit.undo", json!({}));
    assert!(s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch.curves.is_empty());
    run(&mut s, "edit.redo", json!({}));
    assert!(s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch.curves.len() == 1);

    let pristine = s.doc.clone();
    let original_history = (s.undo.len(), s.redo.len(), s.evaluations);
    for request in [
        json!({"command":"sketch.line","params":{"points":[[0,0],[10,0]]},"dimensions":{"length":"Missing + 2"}}),
        json!({"command":"sketch.line","params":{"sketch":made["drawing"],"points":[[0,0],[10,0]]},"dimensions":{"length":25}}),
        json!({"command":"sketch.line","params":{"points":[[0,0],[10,0]]},"dimensions":{"length":"1".repeat(4097)}}),
        json!({"command":"sketch.rectangle.rounded","params":{"p0":[10,10],"p1":[50,30],"radius":30},"dimensions":{"width":40}}),
    ] {
        refused(&mut s, "sketch.draw.dimensioned", request.clone());
        assert!(Arc::ptr_eq(&s.doc, &pristine), "{request}");
        assert_eq!((s.undo.len(), s.redo.len(), s.evaluations), original_history, "{request}");
        assert!(Arc::ptr_eq(&s.model.state(), &model));
    }
}

/// Wrong target and unadmitted record families never borrow active model geometry.
#[test]
fn sheet_batch_modify_refuses_foreign_owner_and_unpersisted_families_atomically() {
    let (mut s, drawing, sketch, _) = sheet_editor();
    let line = run(&mut s, "sketch.line", json!({"points": [[10,10],[50,10]], "connect": false}))["curves"][0].clone();
    let source = s.doc.clone();
    let model = s.model.state();
    let entry = s.active_sheet_sketch.clone();
    let history = (s.undo.len(), s.redo.len(), s.evaluations);
    for (command, params) in [
        ("sketch.move", json!({"sketch":ids::encode(drawing),"entities":[line],"translate":[5,5]})),
        ("sketch.scale", json!({"sketch":ids::encode(drawing),"entities":[line],"factor":2})),
        ("sketch.break", json!({"sketch":ids::encode(drawing),"curve":line,"points":[[30,10]]})),
        ("sketch.trim", json!({"sketch":ids::encode(drawing),"curve":line,"at":[30,10]})),
        ("sketch.drag", json!({"sketch":ids::encode(drawing),"entity":line,"from":[30,10],"to":[40,20]})),
        ("sketch.drag", json!({"entity":line,"from":[30,10],"to":[40,20],"live":true})),
        ("sketch.drag", json!({"entity":line,"from":[30,10],"to":[40,20],"live":"frame"})),
        ("sketch.fillet", json!({"sketch":ids::encode(drawing),"a":line,"b":line,"radius":5})),
        ("sketch.mirror", json!({"sketch":ids::encode(drawing),"entities":[line],"line":line})),
    ] {
        refused(&mut s, command, params.clone());
        assert!(Arc::ptr_eq(&s.doc, &source), "{command}: {params}");
        assert!(Arc::ptr_eq(&s.model.state(), &model), "{command}");
        assert_eq!(s.active_sheet_sketch, entry, "{command}");
        assert_eq!((s.undo.len(), s.redo.len(), s.evaluations), history, "{command}");
    }
    // Association families require a bounded sheet-record contract before they can be enabled.
    for command in ["sketch.offset", "sketch.pattern.circular", "sketch.pattern.rectangular"] {
        assert!(find_command(command).unwrap().allowed(&s).is_err(), "{command} must stay disabled on sheets");
    }
    assert!(s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).is_some());
}

/// R17 2D creator coverage: only existing curve constructors and the paper receiver.
#[test]
fn sheet_batch_new_2d_creators_preserve_paper_records_and_history() {
    for (command, params) in [
        ("sketch.line.midpoint", json!({"mid": [40, 40], "end": [60, 40]})),
        ("sketch.slot.center_point", json!({"center": [60, 50], "end": [80, 50], "width": 8})),
        ("sketch.slot.arc_three_point", json!({"start": [80, 50], "through": [70, 65], "end": [60, 50], "width": 8})),
        ("sketch.slot.arc_center", json!({"center": [60, 50], "start": [80, 50], "end": [60, 70], "width": 8})),
        ("sketch.ellipse", json!({"center": [80, 50], "major": [100, 50], "minor_radius": 10})),
        ("sketch.spline.fit_point", json!({"points": [[40,40],[50,60],[70,55],[85,40]],"connect":false})),
        ("sketch.spline.control_point", json!({"points": [[40,40],[50,60],[70,55],[85,40]],"connect":false})),
        ("sketch.spline.control_point_5", json!({"points": [[40,40],[50,60],[55,65],[70,65],[80,55],[85,40]],"connect":false})),
        ("sketch.conic", json!({"start":[40,40],"end":[90,40],"apex":[65,65],"rho":0.5,"connect":false})),
    ] {
        let (mut s, drawing, sketch, _) = sheet_editor();
        let source = s.doc.clone();
        let model = s.model.state();
        let steps = s.undo.len();
        let evals = s.evaluations;
        let result = run(&mut s, command, params.clone());
        assert!(result["curves"].as_array().is_some_and(|curves| !curves.is_empty()), "{command}: {result}");
        let record = s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch.clone();
        assert!(!record.curves.is_empty(), "{command}");
        assert_eq!((s.undo.len(), s.undo.last().unwrap().part), (steps + 1, Some(drawing)), "{command}");
        assert_eq!((&s.doc.features, &s.doc.params), (&source.features, &source.params), "{command}");
        assert!(Arc::ptr_eq(&model, &s.model.state()), "{command}");
        assert_eq!(s.evaluations, evals, "{command}");
        let reopened = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
        assert_eq!(reopened.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch, record, "{command}");
        run(&mut s, "edit.undo", json!({}));
        assert!(s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch.curves.is_empty(), "{command}");
        run(&mut s, "edit.redo", json!({}));
        assert_eq!(s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch, record, "{command}");
    }
}

/// G1 blends and local line types operate within the same sheet and solver.
#[test]
fn sheet_batch_blend_tangent_arc_centerlines_and_infinite_are_local() {
    let (mut s, drawing, sketch, _) = sheet_editor();
    let a = run(&mut s, "sketch.line", json!({"points":[[30,50],[60,50]],"connect":false,"infer":false}))["curves"][0].as_str().unwrap().to_string();
    let b =
        run(&mut s, "sketch.line", json!({"points":[[100,70],[120,70]],"connect":false,"infer":false}))["curves"][0].as_str().unwrap().to_string();
    let source = s.doc.clone();
    let model = s.model.state();
    let evaluations = s.evaluations;
    let blended = run(&mut s, "sketch.blend_curve", json!({"a":format!("{a}.end"),"b":format!("{b}.start"),"continuity":"g1"}));
    assert!(blended["curves"].as_array().is_some_and(|v| !v.is_empty()), "{blended}");
    let tangent = run(&mut s, "sketch.arc.tangent", json!({"start":format!("{b}.end"),"end":[135,85],"curve":b}));
    assert!(tangent["curves"].as_array().is_some_and(|v| !v.is_empty()), "{tangent}");
    let mid = run(&mut s, "sketch.centerline", json!({"curves":[a],"value":true}));
    assert_eq!(mid["changed"], 1);
    let infinite = run(&mut s, "sketch.infinite", json!({"curves":[a],"value":true}));
    assert_eq!(infinite["changed"], 1);
    let stored = s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch.clone();
    let ci = stored.curve_index(&a).unwrap();
    assert!(stored.curves[ci].infinite && stored.curves[ci].construction);
    assert_eq!((&s.doc.features, &s.doc.params), (&source.features, &source.params));
    assert!(Arc::ptr_eq(&s.model.state(), &model));
    assert_eq!(s.evaluations, evaluations);
    let reopened = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
    assert_eq!(reopened.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch, stored);
    let before = s.doc.clone();
    let undo = (s.undo.len(), s.redo.len(), s.evaluations);
    refused(&mut s, "sketch.blend_curve", json!({"a":format!("{a}.end"),"b":format!("{b}.start"),"continuity":"g4"}));
    assert!(Arc::ptr_eq(&s.doc, &before));
    assert_eq!((s.undo.len(), s.redo.len(), s.evaluations), undo);
}

/// A tangent circle with a radius expression belongs to the sheet, not model parameters.
#[test]
fn sheet_batch_tangent_circle_read_only_source_and_reference_refusal() {
    let (mut s, drawing, sketch, _) = sheet_editor();
    s.doc_mut().set_param("Inset", "1", Some("in"), None).unwrap();
    let h = run(&mut s, "sketch.line", json!({"points":[[20,30],[100,30]],"connect":false,"infer":false}))["curves"][0].clone();
    let v = run(&mut s, "sketch.line", json!({"points":[[30,20],[30,100]],"connect":false,"infer":false}))["curves"][0].clone();
    run(&mut s, "sketch.constraint.fix", json!({"entities":[h,v]}));
    let source = s.doc.clone();
    let model = s.model.state();
    let history = s.undo.len();
    let evals = s.evaluations;
    let r = run(&mut s, "sketch.circle.two_tangent", json!({"curves":[h,v], "radius":"Inset / 5","near":[40,40]}));
    assert!(r["curves"].as_array().is_some_and(|x| x.len() == 1), "{r}");
    assert_eq!((s.undo.len(), s.undo.last().unwrap().part), (history + 1, Some(drawing)));
    let sheet = s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch.clone();
    assert!(sheet.constraints.iter().any(|c| c.param.as_deref() == Some("Inset / 5")));
    assert_eq!((&s.doc.features, &s.doc.params), (&source.features, &source.params));
    assert!(Arc::ptr_eq(&s.model.state(), &model));
    assert_eq!(s.evaluations, evals);
    let reopened = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
    assert_eq!(reopened.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch, sheet);
    let before = s.doc.clone();
    let undo = (s.undo.len(), s.redo.len(), s.evaluations);
    refused(&mut s, "sketch.circle.two_tangent", json!({"curves":[h,v],"radius":"Missing + 1","near":[40,40]}));
    assert!(Arc::ptr_eq(&s.doc, &before));
    assert_eq!((s.undo.len(), s.redo.len(), s.evaluations), undo);
}

/// The known 2D families are enabled for sheets, while text links and patterns remain
/// blocked by the current bounded stored-record contract.
#[test]
fn sheet_batch_command_capability_matrix_refuses_unsafe_record_families() {
    let (s, _, _, _) = sheet_editor();
    for id in [
        "sketch.line.midpoint",
        "sketch.arc.tangent",
        "sketch.circle.two_tangent",
        "sketch.circle.three_tangent",
        "sketch.slot.center_point",
        "sketch.slot.arc_three_point",
        "sketch.slot.arc_center",
        "sketch.ellipse",
        "sketch.spline.fit_point",
        "sketch.spline.control_point",
        "sketch.spline.control_point_5",
        "sketch.conic",
        "sketch.blend_curve",
        "sketch.centerline",
        "sketch.infinite",
        "sketch.rectangle.rounded",
        "sketch.draw.dimensioned",
        "sketch.fillet",
        "sketch.chamfer.equal_distance",
        "sketch.chamfer.distance_angle",
        "sketch.chamfer.two_distance",
        "sketch.trim",
        "sketch.trim.path",
        "sketch.extend",
        "sketch.break",
        "sketch.scale",
        "sketch.move",
        "sketch.mirror",
        "sketch.drag",
    ] {
        assert!(find_command(id).unwrap().allowed(&s).is_ok(), "{id} should accept an active drawing-owned sketch");
    }
    for id in [
        "sketch.text",
        "sketch.edit_text",
        "sketch.offset",
        "sketch.pattern.circular",
        "sketch.pattern.rectangular",
        "sketch.drag_end",
        "sketch.create",
        "sketch.edit",
    ] {
        assert!(find_command(id).unwrap().allowed(&s).is_err(), "{id} is not admitted on a sheet");
    }
}
