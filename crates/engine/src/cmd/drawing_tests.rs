use super::*;
use crate::cmd::{Part, command_specs, find_command};
use crate::tests::Folder;
use solidcraft_doc::drawing::{Units, Updates, template};
use solidcraft_doc::ids::IdSource;
use solidcraft_geom::cell::Projection;
use solidcraft_geom::prelude::*;

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

/// A sheet's general tolerances, as `drawing.inspect` lists its cells.
fn tolerances(made: &Value) -> Value {
    let cells = made["sheets"][0]["title_block_cells"].as_array().unwrap();
    cells.iter().find(|cell| cell["name"] == "general_tolerances").unwrap()["text"].clone()
}

/// A drawing starts from the template named, else its kind's: a part's on ISO A3 with the part
/// note over its general tolerances, an assembly's on ISO A2 with the assembly note, which
/// `drawing.template.list` marks for the design. It keeps the template's projection unless given
/// one. An older name reads as that size's part template; an unknown one is refused with nothing
/// changed, and undo takes the drawing away whole.
#[test]
fn a_drawing_starts_from_a_template() {
    let mut s = design();
    let default = |s: &mut Session| {
        let listed = run(s, "drawing.template.list", json!({}))["templates"].clone();
        let marked: Vec<Value> = listed.as_array().unwrap().iter().filter(|t| t["default"] == json!(true)).cloned().collect();
        (listed, marked)
    };
    let (listed, marked) = default(&mut s);
    let names: Vec<&str> = listed.as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(names, template::BUILT_INS);
    assert_eq!(marked, [json!({"name": "iso-a3-part", "title": "ISO A3", "assembly": false, "default": true})]);
    let made = run(&mut s, "drawing.create", json!({}));
    assert_eq!((&made["sheets"][0]["size_mm"], &made["projection"]), (&json!([420.0, 297.0]), &json!("third")));
    assert_eq!(tolerances(&made), json!(template::PART_NOTE));
    run(&mut s, "drawing.close", json!({}));
    let made = run(&mut s, "drawing.create", json!({"template": "iso-a4-assembly", "projection": "first", "language": "de"}));
    let sheet = &made["sheets"][0];
    assert_eq!((&sheet["size_mm"], &sheet["orientation"]), (&json!([210.0, 297.0]), &json!("portrait")), "the one named wins");
    assert_eq!((&made["projection"], &made["language"], tolerances(&made)), (&json!("first"), &json!("de"), json!(template::ASSEMBLY_NOTE)));
    assert_eq!(open_one(&s).sheets[0].title_block, Some(solidcraft_geom::title_block::default_block()), "the built-in block");
    run(&mut s, "drawing.close", json!({}));
    let made = run(&mut s, "drawing.create", json!({"template": "iso-a1"}));
    assert_eq!((&made["sheets"][0]["size_mm"], tolerances(&made)), (&json!([841.0, 594.0]), json!(template::PART_NOTE)), "an older name");
    let (undo, before) = (s.undo.len(), s.doc.drawings.clone());
    for old in ["iso-b2", "iso-a3-parts", "iso-a5-part"] {
        assert!(refused(&mut s, "drawing.create", json!({"template": old})).contains(&format!("no template `{old}`")), "{old}");
    }
    assert_eq!((s.undo.len(), &s.doc.drawings), (undo, &before), "nothing changed");
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.drawings.len(), 2, "undo takes the drawing away whole");

    // An assembly of two components: A2, with the assembly note.
    let mut s = Session::default();
    for (name, corner) in [("Bracket", [0, 0, 0]), ("Pin", [100, 0, 0])] {
        run(&mut s, "component.create", json!({"name": name}));
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": corner, "body_name": name}));
        run(&mut s, "component.activate", json!({"component": "root"}));
    }
    assert_eq!(default(&mut s).1.iter().map(|t| t["name"].clone()).collect::<Vec<_>>(), [json!("iso-a2-assembly")]);
    let made = run(&mut s, "drawing.create", json!({}));
    assert_eq!((&made["sheets"][0]["size_mm"], &made["projection"]), (&json!([594.0, 420.0]), &json!("third")));
    assert_eq!(tolerances(&made), json!(template::ASSEMBLY_NOTE));
}

/// A one-part STEP draws as a part (#19753), rounded or not; two parts, or one beside a body, as an assembly.
#[test]
fn an_opened_step_part_draws_from_the_part_template() {
    let folder = Folder::new("drawing-step-part");
    let step = folder.path("plate.step");
    run(&mut design(), "file.export", json!({"path": step}));
    let made = |s: &mut Session| {
        let made = run(s, "drawing.create", json!({}));
        run(s, "drawing.close", json!({}));
        (made["sheets"][0]["size_mm"].clone(), tolerances(&made))
    };
    let part = (json!([420.0, 297.0]), json!(template::PART_NOTE));
    let assembly = (json!([594.0, 420.0]), json!(template::ASSEMBLY_NOTE));

    let mut s = Session::default();
    run(&mut s, "doc.open", json!({"path": step}));
    assert_eq!(s.doc.occurrences.len(), 1);
    assert_eq!(made(&mut s), part, "one imported part");
    run(&mut s, "solid.fillet", json!({"edges": [[40, 15, 10]], "radius": 2}));
    run(&mut s, "solid.hole", json!({"position": [10, 10, 10], "direction": [0, 0, -1], "diameter": 4, "depth": 5}));
    assert!(s.doc.features.iter().all(|f| f.component != 0), "a round and a hole go in their body's component");
    assert_eq!(made(&mut s), part, "one imported part, rounded and drilled with the root active");

    run(&mut s, "file.insert_step", json!({"path": step}));
    assert_eq!(made(&mut s), assembly, "two imported parts");

    let mut s = design();
    run(&mut s, "file.insert_step", json!({"path": step}));
    assert_eq!(made(&mut s), assembly, "an imported part beside the design's own body");
}

#[test]
fn a_drawing_with_standard_views_is_one_model_undo_step() {
    let mut s = design();
    let undo_steps = s.undo.len();
    let refused = refused(
        &mut s,
        "drawing.create",
        json!({
            "standard_views": true,
            "first_sheet_paper":{"size":{"custom":{"sides_mm":[30.0,30.0]}}},
            "omit_first_sheet_title_block": true
        }),
    );
    assert!(refused.contains("fit the sheet"), "{refused}");
    assert!(s.open_drawing.is_none());
    assert!(s.doc.drawings.is_empty());
    assert_eq!(s.undo.len(), undo_steps);
    let made = run(&mut s, "drawing.create", json!({"standard_views": true}));
    assert_eq!(made["sheets"][0]["views"].as_array().unwrap().len(), 4);
    assert_eq!(s.undo.len(), undo_steps + 1);
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "edit.undo", json!({}));
    assert!(s.doc.drawings.is_empty());
}

#[test]
fn first_sheet_overrides_apply_in_one_undo_step() {
    let mut s = design();
    let undo_steps = s.undo.len();
    let made = run(
        &mut s,
        "drawing.create",
        json!({
            "template":"iso-a4-part",
            "first_sheet_name":"Detail",
            "first_sheet_paper":{"size":{"custom":{"sides_mm":[350.0,500.0]}},"orientation":"portrait"},
            "omit_first_sheet_title_block":true
        }),
    );
    assert_eq!(made["sheets"][0]["size_mm"], json!([350.0, 500.0]));
    let sheet = &open_one(&s).sheets[0];
    assert_eq!(sheet.name, "Detail");
    assert!(sheet.border.is_some());
    assert!(sheet.title_block.is_none());
    assert!(sheet.title_block_overrides.is_empty());
    assert_eq!(s.undo.len(), undo_steps + 1);
}

/// A template applied to the drawing open: a dry run answers what would change and changes nothing;
/// applying it is one undo step, and applying it again none. The views the new paper leaves
/// outside are listed and stay where they were.
#[test]
fn a_template_applied_restyles_the_sheets_and_moves_nothing() {
    let (mut s, [front, right, top]) = crate::cmd::drawing_view::tests::three_views();
    let before = open_one(&s);
    let (steps, sheet) = (s.undo.len(), ids::encode(before.sheets[0].id));
    let dry = run(&mut s, "drawing.template.apply", json!({"template": "iso-a4-part", "dry_run": true}));
    assert_eq!(dry["changed"], json!([{"sheet": sheet, "paper": true, "border": false, "title_block": false, "lost": []}]));
    let outside: Vec<&str> = dry["outside"].as_array().unwrap().iter().map(|v| v["view"].as_str().unwrap()).collect();
    assert!(outside.contains(&ids::encode(right).as_str()) && outside.contains(&ids::encode(top).as_str()), "{dry}");
    assert!(!outside.contains(&ids::encode(front).as_str()), "{dry}");
    assert_eq!((open_one(&s), s.undo.len()), (before.clone(), steps), "a dry run changes nothing");
    let applied = run(&mut s, "drawing.template.apply", json!({"template": "iso-a4-part"}));
    assert_eq!((&applied["changed"], &applied["outside"], &applied["dry_run"]), (&dry["changed"], &dry["outside"], &json!(false)));
    let after = open_one(&s);
    assert_eq!(after.sheets[0].sheet, template::built_in("iso-a4-part").unwrap().unwrap().sheets[0].sheet);
    assert_eq!(after.views, before.views, "nothing moves");
    assert_eq!(s.undo.len(), steps + 1);
    let again = run(&mut s, "drawing.template.apply", json!({"template": "iso-a4-part"}));
    assert_eq!((again["changed"].clone(), s.undo.len()), (json!([]), steps + 1), "nothing to change, no step");
    for (p, why) in [
        (json!({"template": "iso-b2"}), "no template `iso-b2`"),
        (json!({}), "`template` must be a template's name"),
        (json!({"template": "iso-a3-part", "dry_run": "yes"}), "`dry_run` is true or false"),
    ] {
        assert!(refused(&mut s, "drawing.template.apply", p.clone()).contains(why), "{p}");
    }
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(open_one(&s).sheets, before.sheets, "undo puts the paper back");
}

/// A drawing saved as a template file starts new drawings and restyles others, with nothing of its
/// model: a copy of the built-in with a company's changes is a template of its own. A later save
/// changes no drawing made from it. Only a .json path is taken, and only inside
/// the folders an agent may use while a scope is active.
#[test]
fn a_template_file_saves_and_starts_drawings() {
    let (mut s, _) = crate::cmd::drawing_view::tests::three_views();
    let folder = Folder::new("drawing-template");
    let path = folder.path("company-a3.scdwg");
    // The company's name written into the built-in block, and its general tolerances typed over it.
    let mut block = solidcraft_geom::title_block::default_block();
    let owner = block.cells.iter_mut().find(|cell| cell.name == "legal_owner").unwrap();
    let solidcraft_geom::title_block::CellKind::Text(owner) = &mut owner.kind else { panic!("the legal owner cell shows text") };
    owner.content = solidcraft_geom::cell::Content(vec![solidcraft_geom::cell::Part::Text("Acme Ltd".into())]);
    let changes = json!({"sheet": "Sheet 1", "title_block": block, "title_block_overrides": {"general_tolerances": "ISO 2768-m"}});
    run(&mut s, "drawing.sheet.set", changes);
    let saved = run(&mut s, "drawing.template.save", json!({"path": path}));
    assert_eq!(saved["bytes"].as_u64(), Some(std::fs::metadata(&path).unwrap().len()));
    let first = open_one(&s);
    run(&mut s, "drawing.close", json!({}));
    let made = run(&mut s, "drawing.create", json!({"name": "From the file", "template": path}));
    let from_file = open_one(&s);
    assert!(from_file.views.is_empty() && from_file.sheets[0].id != first.sheets[0].id, "{made}");
    assert_eq!((from_file.sheets[0].sheet, &from_file.sheets[0].title_block), (first.sheets[0].sheet, &first.sheets[0].title_block));
    assert_eq!(from_file.sheets[0].title_block_overrides, first.sheets[0].title_block_overrides);
    let cells = run(&mut s, "drawing.inspect", json!({}))["sheets"][0]["title_block_cells"].clone();
    let shown = |name: &str| cells.as_array().unwrap().iter().find(|cell| cell["name"] == name).unwrap()["text"].clone();
    assert_eq!((shown("legal_owner"), shown("general_tolerances")), (json!("Acme Ltd"), json!("ISO 2768-m")), "the company's copy");
    // A later save of another format changes no drawing made from the file.
    run(&mut s, "drawing.template.apply", json!({"template": "iso-a4-part"}));
    run(&mut s, "drawing.template.save", json!({"path": path}));
    let other = s.doc.drawings.iter().find(|d| d.id == first.id).unwrap().clone();
    assert_eq!(other.sheets, first.sheets, "the first drawing kept its own copy");
    let applied = run(&mut s, "drawing.template.apply", json!({"template": path, "dry_run": true}));
    assert_eq!(applied["changed"], json!([]), "the file now holds this drawing's own format: {applied}");
    for (command, p, why) in [
        ("drawing.template.save", json!({"path": folder.path("company.txt")}), "`path` must be a .json or .scdwg file"),
        ("drawing.template.save", json!({}), "`path` must be a .json or .scdwg file"),
        ("drawing.create", json!({"template": folder.path("missing.json")}), "template `"),
        ("drawing.create", json!({"template": "company"}), "no template `company`"),
    ] {
        assert!(refused(&mut s, command, p.clone()).contains(why), "{command} {p}");
    }
    let plain = folder.path("plain.json");
    std::fs::write(&plain, first.to_json().unwrap()).unwrap();
    assert!(refused(&mut s, "drawing.create", json!({"template": plain})).contains("a template draws no design"));
    // While a scope is active, only its folders.
    let (inside, outside) = (Folder::new("drawing-template-in"), Folder::new("drawing-template-out"));
    solidcraft_io::vfs::scoped(Some(solidcraft_io::vfs::PathScope::new([inside.0.clone()])), || {
        run(&mut s, "drawing.template.save", json!({"path": inside.path("ok.json")}));
        let refusal = refused(&mut s, "drawing.template.save", json!({"path": outside.path("no.json")}));
        assert!(refusal.contains("outside the folders this agent may use"), "{refusal}");
        let refusal = refused(&mut s, "drawing.create", json!({"template": path}));
        assert!(refusal.contains("outside the folders this agent may use"), "{refusal}");
    });
    // A .json name gets the .scdwg file beside it, inside the scope as the name asked.
    assert!(std::path::Path::new(&inside.path("ok.scdwg")).is_file() && !std::path::Path::new(&outside.path("no.scdwg")).exists());
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

/// A drawing of the design: one A3 sheet with the ISO border and the built-in title block, the design
/// its one source, kept in the design and opened. Making it is one step of the model, and evaluates nothing.
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
    let built_in = template::built_in(template::default_for(false)).unwrap().unwrap();
    assert_eq!(d.sheets[0].title_block, built_in.sheets[0].title_block, "the built-in block");
    assert_eq!(sheet["title_block"]["size_mm"], json!([180.0, 49.0]));
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

/// A drawing of another design's file (`source`) may be made in an empty design. Refused with
/// nothing changed: a path the record can't take or longer than 4,096 bytes, a network location
/// not allowed (judged from the text first, whatever the file's name, with nothing asked of the
/// disk), a file that isn't a design, the design's own file, and a missing file. Its views wait for
/// views of a linked design.
#[test]
fn a_drawing_of_another_designs_file_is_made_only_of_a_design_there() {
    let folder = Folder::new("drawing-source-create");
    run(&mut design(), "file.save", json!({"path": folder.path("plate.sc")}));
    std::fs::write(folder.path("part.step"), b"ISO-10303-21;").unwrap();
    std::fs::create_dir_all(folder.path("sub")).unwrap();
    let mut s = Session::default();
    run(&mut s, "file.save", json!({"path": folder.path("case.sc")}));
    let before = s.doc.clone();
    let fits = "x".repeat(4093) + ".sc";
    let calls = solidcraft_io::vfs::file_calls();
    for (source, says) in [(r"\\fileserver\share\p.sc".to_string(), "fileserver"), (r"\\fileserver\share\p.step".to_string(), "fileserver")] {
        let e = refused(&mut s, "drawing.create", json!({"source": source}));
        assert!(e.contains(says), "{source}: {e}");
        assert!(Arc::ptr_eq(&s.doc, &before) && s.open_drawing.is_none(), "{source}: nothing changed");
    }
    assert_eq!(solidcraft_io::vfs::file_calls(), calls, "a network location asked the file system");
    let path_rule = "not empty, not only spaces, without control characters";
    for (source, says) in [
        (String::new(), path_rule),
        ("  ".into(), path_rule),
        ("a\0b.sc".into(), path_rule),
        ("x".repeat(4094) + ".sc", "at most 4096 bytes"),
        (folder.path("part.step"), "not a .sc design"),
        (folder.path("case.sc"), "the design it is in"),
        (folder.path("sub/../case.sc"), "the design it is in"),
        (folder.path("gone.sc"), "not found"),
    ] {
        let e = refused(&mut s, "drawing.create", json!({"source": source}));
        assert!(e.contains(says), "{source:?}: {e}");
        assert!(Arc::ptr_eq(&s.doc, &before) && s.open_drawing.is_none(), "{source:?}: nothing changed");
    }
    // 4,096 bytes is still a path to look for.
    let e = refused(&mut s, "drawing.create", json!({"source": fits}));
    assert!(!e.contains("at most 4096 bytes"), "{e}");
    let made = run(&mut s, "drawing.create", json!({"source": folder.path("plate.sc")}));
    assert_eq!(made["sources"], json!([{"id": made["sources"][0]["id"], "path": "plate.sc"}]), "kept beside the design");
    assert_eq!(open_one(&s).own_source(), None);
    let drawn = s.doc.clone();
    let e = refused(&mut s, "drawing.view.base", json!({"sheet": made["sheets"][0]["id"]}));
    assert!(e.contains("not supported yet") && e.contains("linked design"), "{e}");
    assert!(Arc::ptr_eq(&s.doc, &drawn));
}

/// Which design a source means has one answer (`Session::drawn`): the design the drawing is in for
/// its own source; for another design's file, until it can be read, a refusal that changes nothing.
/// So a sheet never documents a linked source (its title block would show this design's name and
/// mass as the other's), a parts list never lists one, and a sheet a file made that documents one
/// shows its component's fields as it shows none, never this design's.
#[test]
fn a_linked_source_is_never_read_as_the_design_the_drawing_is_in() {
    use solidcraft_doc::drawing::SheetComponent;
    let folder = Folder::new("drawing-source-drawn");
    run(&mut design(), "file.save", json!({"path": folder.path("plate.sc")}));
    let mut s = design();
    run(&mut s, "file.save", json!({"path": folder.path("case.sc")}));
    let made = run(&mut s, "drawing.create", json!({"name": "Of the plate", "source": folder.path("plate.sc")}));
    let (sheet, linked) = (made["sheets"][0]["id"].clone(), made["sources"][0]["id"].clone());
    let before = s.doc.clone();
    for (id, p) in [
        ("drawing.sheet.set", json!({"sheet": sheet, "documents": {"source": linked}})),
        ("drawing.sheet.add", json!({"documents": {"source": linked, "component": 0}})),
        ("drawing.table.add", json!({"sheet": sheet, "kind": "parts_list"})),
        ("drawing.view.base", json!({"sheet": sheet})),
    ] {
        let e = refused(&mut s, id, p);
        assert!(e.contains("not supported yet") && e.contains("linked design"), "{id}: {e}");
        assert!(Arc::ptr_eq(&s.doc, &before), "{id}: nothing changed");
    }
    // A sheet a file made that documents the linked design shows what a sheet that documents nothing shows.
    let cells = |s: &mut Session| run(s, "drawing.inspect", json!({}))["sheets"][0]["title_block_cells"].clone();
    let nothing = cells(&mut s);
    let mut d = (*open_one(&s)).clone();
    let source = ids::decode(linked.as_str().unwrap()).unwrap();
    Arc::make_mut(&mut d.sheets[0]).documents = Some(SheetComponent { source, component: 0 });
    d.validate().unwrap();
    s.doc_mut().put_drawing(Arc::new(d));
    assert_eq!(cells(&mut s), nothing);
    // Its own source reads the design it is in.
    run(&mut s, "drawing.close", json!({}));
    let made = run(&mut s, "drawing.create", json!({}));
    let own = json!({"source": made["sources"][0]["id"]});
    run(&mut s, "drawing.sheet.set", json!({"sheet": made["sheets"][0]["id"], "documents": own}));
    assert_ne!(cells(&mut s), nothing, "the design's own fields");
}

/// `drawing.create` and `drawing.set` refuse a key they don't take, as every other drawing command
/// does, naming it, with nothing changed: a typo is never taken for a setting left as it was. Every
/// key they take still takes, in one call.
#[test]
fn an_unknown_key_is_refused_and_changes_nothing() {
    let mut s = design();
    let before = s.doc.clone();
    for (p, key) in [(json!({"nmae": "Plate"}), "nmae"), (json!({"name": "Plate", "profile": "iso_129_1"}), "profile")] {
        let e = refused(&mut s, "drawing.create", p.clone());
        assert!(e.contains(&format!("unknown field `{key}`")), "{p}: {e}");
        assert!(Arc::ptr_eq(&s.doc, &before) && s.open_drawing.is_none(), "{p}: nothing changed");
    }
    let every =
        json!({"name": "Plate", "template": "iso-a4-part", "language": "de", "units": "in", "projection": "first", "line_group": "0.7", "font": ""});
    run(&mut s, "drawing.create", every);
    let made = open_one(&s);
    assert_eq!((made.name.as_str(), made.language.as_str(), made.projection, made.line_group), ("Plate", "de", Projection::First, LineGroup::Mm0_7));
    let paper = made.sheets[0].sheet.size_mm().unwrap();
    assert_eq!((&made.units, paper.x.max(paper.y)), (&Units::Fixed { unit: "in".into() }, 297.0), "A4, from the template");
    let steps = s.undo.len();
    for (p, key) in [(json!({"line_groups": "0.5"}), "line_groups"), (json!({"font": "Liberation Sans", "fnot": 1}), "fnot")] {
        let e = refused(&mut s, "drawing.set", p.clone());
        assert!(e.contains(&format!("unknown field `{key}`")), "{p}: {e}");
        assert!(Arc::ptr_eq(&open_one(&s), &made) && s.undo.len() == steps, "{p}: nothing changed");
    }
    let every = json!({"name": "Plate 2", "language": "en-GB", "units": "follow_model", "projection": "third", "updates": "deferred", "line_group": "1", "font": "Liberation Sans", "revision_numbering": {"start": 0}});
    run(&mut s, "drawing.set", every);
    let set = open_one(&s);
    assert_eq!((set.name.as_str(), set.language.as_str(), &set.units, set.projection), ("Plate 2", "en-GB", &Units::FollowModel, Projection::Third));
    assert_eq!(
        (set.updates, set.line_group, set.font.as_str(), set.revisions.numbering.start),
        (Updates::Deferred, LineGroup::Mm1, "Liberation Sans", 0)
    );
    assert_eq!(s.undo.len(), steps + 1, "one undo step");
}

/// The model's commands run with a drawing open or not, and the drawing's only while one is:
/// running a command and listing it say so. A model step taken with the drawing open is the
/// model's (undo in the drawing leaves it), and the drawing stays open.
#[test]
fn model_and_drawing_commands_each_run_on_their_own_part() {
    let mut s = design();
    for id in ["drawing.inspect", "drawing.set", "drawing.properties", "drawing.properties.set", "drawing.properties.remove"] {
        assert!(refused(&mut s, id, json!({"name": "X", "key": "author", "value": "X"})).contains("open a drawing first"), "{id}");
    }
    run(&mut s, "drawing.create", json!({}));
    let open = s.open_drawing;
    let boxed = json!({"length": 5, "width": 5, "height": 5});
    let features = s.doc.features.len();
    run(&mut s, "solid.box", boxed.clone());
    assert_eq!(s.doc.features.len(), features + 1, "the model changes with a drawing open");
    assert_eq!(s.open_drawing, open, "and the drawing stays open");
    assert_eq!(s.undo.last().unwrap().part, None, "its step is the model's");
    let enabled = |s: &mut Session, id: &str| {
        let listed = run(s, "engine.commands", json!({}));
        listed.as_array().unwrap().iter().find(|c| c["id"] == id).map(|c| c["enabled"] == true).unwrap()
    };
    assert!(enabled(&mut s, "solid.box") && enabled(&mut s, "drawing.set"));
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

/// A drawing's views follow the model automatically, or only on `drawing.update` when deferred:
/// shown by `drawing.inspect`, changed by `drawing.set` as one undo step, saved with the design only
/// when deferred, and anything else refused.
#[test]
fn a_drawings_updates_are_automatic_or_deferred() {
    let folder = Folder::new("drawing-updates");
    let mut s = design();
    assert_eq!(run(&mut s, "drawing.create", json!({}))["updates"], "automatic");
    let saved = serde_json::to_value(&*open_one(&s)).unwrap();
    assert!(saved.get("updates").is_none(), "automatic isn't saved: {saved}");
    let steps = s.undo.len();
    assert_eq!(run(&mut s, "drawing.set", json!({"updates": "deferred"}))["updates"], "deferred");
    assert_eq!(s.undo.len(), steps + 1);
    run(&mut s, "file.save", json!({"path": folder.path("plate.solidcraft")}));
    let mut opened = Session::default();
    run(&mut opened, "doc.open", json!({"path": folder.path("plate.solidcraft")}));
    assert_eq!(opened.doc.drawings[0].updates, drawing::Updates::Deferred);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(run(&mut s, "drawing.inspect", json!({}))["updates"], "automatic");
    for bad in [json!("sometimes"), json!("Deferred"), json!(true), json!(null)] {
        assert!(refused(&mut s, "drawing.set", json!({"updates": bad})).contains("automatic or deferred"), "{bad}");
    }
}

/// A drawing's line group is one of ISO 128's, 0.5 when nothing chooses: shown by
/// `drawing.inspect` beside each sheet's own, chosen by `drawing.create`, changed by `drawing.set`
/// as one undo step, saved with the design, and anything else refused.
#[test]
fn a_drawings_line_group_is_set_as_one_undo_step_and_saved() {
    let folder = Folder::new("drawing-line-group");
    let mut s = design();
    let made = run(&mut s, "drawing.create", json!({}));
    assert_eq!((&made["line_group"], &made["sheets"][0]["line_group"]), (&json!("0.5"), &Value::Null));
    let steps = s.undo.len();
    assert_eq!(run(&mut s, "drawing.set", json!({"line_group": "0.7"}))["line_group"], "0.7");
    assert_eq!(s.undo.len(), steps + 1);
    run(&mut s, "file.save", json!({"path": folder.path("plate.solidcraft")}));
    let mut opened = Session::default();
    run(&mut opened, "doc.open", json!({"path": folder.path("plate.solidcraft")}));
    assert_eq!(opened.doc.drawings[0].line_group, LineGroup::Mm0_7);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(run(&mut s, "drawing.inspect", json!({}))["line_group"], "0.5");
    for bad in [json!("0.6"), json!(0.5), json!("Mm0_5"), json!(null)] {
        assert!(refused(&mut s, "drawing.set", json!({"line_group": bad})).ends_with(LINE_GROUP_REFUSED), "{bad}");
    }
    assert_eq!(run(&mut design(), "drawing.create", json!({"line_group": "1.4"}))["line_group"], "1.4");
}

const LINE_GROUP_REFUSED: &str = "`line_group` must be one of 0.25, 0.35, 0.5, 0.7, 1, 1.4 or 2";

/// A line group that isn't one of ISO 128's is refused in one wording, after its key, whether the
/// parameters are checked or not: a drawing's, a sheet's or a view's.
#[test]
fn a_line_group_outside_iso_128_is_refused_in_one_wording() {
    for level in [crate::schema::ParamCheck::Off, crate::schema::ParamCheck::Strict] {
        let (mut s, [front, ..]) = crate::cmd::drawing_view::tests::three_views();
        s.param_check = level;
        let view = json!({"view": ids::encode(front), "set": {"line_group": "0.6"}});
        for (id, p) in [
            ("drawing.create", json!({"line_group": "0.6"})),
            ("drawing.set", json!({"line_group": "0.6"})),
            ("drawing.sheet.add", json!({"line_group": "0.6"})),
            ("drawing.sheet.set", json!({"sheet": "Sheet 1", "line_group": "0.6"})),
            ("drawing.view.edit", view.clone()),
        ] {
            let words = if id == "drawing.view.edit" { format!("`set.{}", &LINE_GROUP_REFUSED[1..]) } else { LINE_GROUP_REFUSED.to_string() };
            assert!(refused(&mut s, id, p).ends_with(&words), "{level:?} {id}");
        }
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
    // The font is checked as given, before it is trimmed, and again as the drawing keeps it: a
    // control character or an overlong name is refused even where trimming would hide it.
    let (drawings, steps) = (s.doc.drawings.len(), s.undo.len());
    for id in ["drawing.create", "drawing.set"] {
        let file = refused(&mut s, id, json!({"font": "a.ttf"}));
        assert_eq!(file, format!("invalid parameters for `{id}`: invalid value: `a.ttf` is a file: drawings name fonts by family, so they travel"));
        let one_line = format!("invalid parameters for `{id}`: invalid value: a font family's name is at most 1024 bytes on one line");
        assert_eq!(refused(&mut s, id, json!({"font": " \tArial\n"})), one_line);
        assert_eq!(refused(&mut s, id, json!({"font": format!("Arial{}", " ".repeat(1024))})), one_line);
    }
    assert_eq!((s.doc.drawings.len(), s.undo.len()), (drawings, steps), "nothing refused is kept");
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

/// A drawing's source of another design's file follows Save As as a component link does (#6860):
/// in memory the path is whole, so Save As into other folders, undo, redo, a checkpoint and a
/// recovered autosave all still name the same file, and saving never rewrites the design in memory
/// or its history; the file keeps the path relative to the design's folder, in both forms.
#[test]
fn save_as_keeps_drawing_sources_through_undo_redo_and_checkpoints() {
    use crate::cmd::design_link::{absolute, resolve};
    for on_copy in [false, true] {
        let folder = Folder::new(&format!("drawing-source-save-as-{on_copy}"));
        let dir = folder.0.clone();
        std::fs::create_dir_all(dir.join("parts")).unwrap();
        std::fs::create_dir_all(dir.join("asm/sub/deeper")).unwrap();
        let part = dir.join("parts/plate.sc");
        run(&mut design(), "file.save", json!({"path": part}));
        let case = dir.join("asm/case.sc");
        let mut s = Session::default();
        run(&mut s, "drawing.create", json!({"source": part}));
        run(&mut s, "drawing.close", json!({}));
        let on_disk = |path: &std::path::Path| {
            let disk = solidcraft_io::read_design(&std::fs::read(path).unwrap()).unwrap();
            disk.drawings[0].sources[0].path.clone().unwrap()
        };
        // As `drawing.inspect` lists it (as the file keeps it), and the file it names.
        let source = |s: &mut Session| {
            let id = s.doc.drawings[0].id;
            let listed = run(s, "drawing.open", the(id))["sources"][0]["path"].as_str().unwrap().to_string();
            run(s, "drawing.close", json!({}));
            let file = absolute(&resolve(s.path.as_deref(), &listed));
            (listed, file)
        };
        run(&mut s, "file.save", json!({"path": case}));
        assert_eq!(on_disk(&case), "../parts/plate.sc");
        // Opening starts from the relative path written to disk.
        run(&mut s, "doc.open", json!({"path": case}));
        assert_eq!(source(&mut s), ("../parts/plate.sc".to_string(), absolute(&part)));
        run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5}));
        let checkpoint = s.checkpoint();
        run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [20, 0, 0]}));
        run(&mut s, "edit.undo", json!({}));
        let before = s.doc.clone();
        let undo_before = s.undo.last().unwrap().doc.clone();
        for (sub, kept) in [("asm/sub", "../../parts/plate.sc"), ("asm/sub/deeper", "../../../parts/plate.sc")] {
            let path = dir.join(sub).join("case.sc");
            if on_copy {
                let mut copy = s.scratch();
                run(&mut copy, "file.save_as", json!({"path": path}));
                s.adopt_save(&copy);
            } else {
                run(&mut s, "file.save_as", json!({"path": path}));
            }
            assert_eq!(on_disk(&path), kept, "{on_copy}, {sub}");
            for command in ["edit.redo", "edit.undo", "edit.undo", "edit.redo"] {
                run(&mut s, command, json!({}));
                assert_eq!(source(&mut s), (kept.to_string(), absolute(&part)), "{on_copy}, {sub}, {command}");
            }
        }
        assert!(Arc::ptr_eq(&s.doc, &before), "saving does not rewrite the live document");
        assert!(Arc::ptr_eq(&s.undo.last().unwrap().doc, &undo_before), "saving does not copy the edit history");
        s.restore_checkpoint(checkpoint).unwrap();
        assert_eq!(source(&mut s).1, absolute(&part));
        let path = s.path.clone().unwrap();
        for format in ["json", "container"] {
            run(&mut s, "file.save", json!({"format": format}));
            assert_eq!(on_disk(Path::new(&path)), "../../../parts/plate.sc", "{format}");
            run(&mut s, "doc.open", json!({"path": path}));
            assert_eq!(source(&mut s).1, absolute(&part), "{format}");
        }
        // A recovered autosave holds the whole path and still names the file.
        let recovery = dir.join("recovery");
        let mut autosaver = crate::recovery::Autosaver::new(&recovery, std::time::Duration::ZERO).unwrap();
        run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [40, 0, 0]}));
        assert!(autosaver.save(&s).unwrap());
        drop(autosaver);
        let entry = crate::recovery::orphans(&recovery).pop().unwrap();
        let mut recovered = Session::default();
        run(&mut recovered, "doc.recover", json!({"dir": recovery, "id": entry.id}));
        assert_eq!(source(&mut recovered).1, absolute(&part));
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
    // `drawing.create`'s and `drawing.set`'s other keys.
    let keys = [&keys[..], &["template", "projection", "updates", "line_group", "font", "revision_numbering", "source"]].concat();
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

/// Inspect lists each sheet's views, each base view followed by those projected from it whichever way
/// the ids sort, with what it is called and what is in it. A view whose parent is on another sheet
/// heads its own list and still names the parent; a sheet with none lists none. Nothing is evaluated.
#[test]
fn inspect_lists_each_sheets_views_in_order() {
    use solidcraft_doc::drawing::balloon::Balloon;
    use solidcraft_doc::drawing::dimension::ModelEdge;
    use solidcraft_doc::drawing::view::ViewPlace;
    let (mut s, [front, right, top]) = crate::cmd::drawing_view::tests::three_views();
    // A second view projected from the front, to its left, and a balloon in the right view.
    let mut d = (**s.drawing("test").unwrap()).clone();
    let mut source = ids::IdSource::seeded(4894);
    let left = d.allocate_id(&mut source).unwrap();
    let balloon = d.allocate_id(&mut source).unwrap();
    let kind = ViewKind::Projected { parent: front, side: ProjectedSide::Left };
    let sheet = d.sheets[0].id;
    d.add_views([Arc::new(View {
        id: left,
        sheet,
        kind,
        place: ViewPlace::Aligned(50.0),
        settings: Default::default(),
        reference_shown_percent: 0.0,
        label: None,
    })]);
    // Inspect only lists it, so the part it numbers needn't be in the design.
    let edge = ModelEdge { body: "Block".into(), origin_feature: s.doc.features[0].id, name: "edge".into(), path: vec![1] };
    d.balloons.add([Arc::new(Balloon { id: balloon, view: right, edge, at: [20.0, 20.0], parts_list: None })]);
    d.validate().unwrap();
    s.doc_mut().put_drawing(Arc::new(d));
    let [front_id, right_id, top_id, left_id] = [front, right, top, left].map(ids::encode);
    let dimension = run(&mut s, "drawing.dimension.add", json!({"view": front_id, "kind": {"length": {"edge": [50, 0, 20]}}}));
    let (features, evaluations) = (s.doc.features.clone(), s.evaluations);
    let reply = run(&mut s, "drawing.inspect", json!({}));
    let mut expected =
        vec![json!({"id": front_id, "kind": "base", "label": "Front", "dimensions": [dimension["record"]["id"]], "balloons": [], "notes": []})];
    let right_and_top = [
        json!({"id": right_id, "kind": "projected", "parent": front_id, "side": "right", "label": "Right of Front", "dimensions": [], "balloons": [ids::encode(balloon)], "notes": []}),
        json!({"id": top_id, "kind": "projected", "parent": right_id, "side": "above", "label": "Above Right of Front", "dimensions": [], "balloons": [], "notes": []}),
    ];
    let left_entry = json!({"id": left_id, "kind": "projected", "parent": front_id, "side": "left", "label": "Left of Front", "dimensions": [], "balloons": [], "notes": []});
    // The front's two views in id order, each followed by what is projected from it.
    if right < left {
        expected.extend(right_and_top);
        expected.push(left_entry);
    } else {
        expected.push(left_entry);
        expected.extend(right_and_top);
    }
    assert_eq!(reply["sheets"][0]["views"], json!(expected));
    assert_eq!((s.evaluations, &s.doc.features), (evaluations, &features));

    let other = run(&mut s, "drawing.sheet.add", json!({"name": "Other"}))["sheet"].clone();
    let lists = |s: &mut Session| {
        let reply = run(s, "drawing.inspect", json!({}));
        let ids_of = |at: usize| reply["sheets"][at]["views"].as_array().unwrap().iter().map(|v| v["id"].clone()).collect::<Vec<_>>();
        (ids_of(0), ids_of(1), reply["sheets"][1]["views"][0].clone())
    };
    assert_eq!(lists(&mut s).1, Vec::<Value>::new(), "{other}");
    run(&mut s, "drawing.view.move_to_sheet", json!({"view": top_id, "sheet": "Other"}));
    let (here, there, moved) = lists(&mut s);
    let staying: Vec<Value> = expected.iter().map(|v| v["id"].clone()).filter(|id| *id != json!(top_id)).collect();
    assert_eq!((here, there), (staying, vec![json!(top_id)]));
    assert_eq!((&moved["parent"], &moved["label"]), (&json!(right_id), &json!("Above Right of Front")), "{moved}");
}

/// Inspect lists everything on each sheet, from the one rule the Browser and a sheet's delete use
/// ([`Drawing::on_sheet`]): its views, a note in a view under its view, a note on the sheet with the
/// sheet, and the sheet's tables, images and revision tags, each on its own sheet only. An empty sheet
/// lists nothing, a deleted object is gone, and nothing is evaluated.
#[test]
fn inspect_lists_everything_on_each_sheet() {
    let (mut s, [front, ..]) = crate::cmd::drawing_view::tests::three_views();
    let folder = Folder::new("drawing-inspect-sheets");
    let picture = folder.path("logo.png");
    std::fs::write(&picture, crate::drawing_pictures::tests::png(4, 2)).unwrap();
    let row = run(&mut s, "drawing.revision.add", json!({"set": {}}))["row"].clone();
    let first = ids::encode(s.drawing("test").unwrap().sheets[0].id);
    let other = run(&mut s, "drawing.sheet.add", json!({"name": "Other"}))["sheet"].as_str().unwrap().to_string();
    run(&mut s, "drawing.sheet.add", json!({"name": "Empty"}));
    // One of each on a sheet: its id by kind.
    let on = |s: &mut Session, sheet: &str| {
        let note = |content: &str| json!({"anchor": {"sheet": sheet}, "at": [20, 20], "content": content});
        [
            run(s, "drawing.note.add", note("General"))["note"].clone(),
            run(s, "drawing.table.add", json!({"sheet": sheet, "kind": "parts_list"}))["table"].clone(),
            run(s, "drawing.image.add", json!({"sheet": sheet, "file": picture, "rect_mm": [10, 10, 30, 20]}))["image"].clone(),
            run(s, "drawing.revision.tag.add", json!({"row": row, "sheet": sheet, "at": [40.0, 40.0]}))["tag"].clone(),
        ]
    };
    let here = on(&mut s, &first);
    let there = on(&mut s, &other);
    let in_view = json!({"anchor": {"view": ids::encode(front)}, "at": [60, 40], "content": "Break"});
    let in_view = run(&mut s, "drawing.note.add", in_view)["note"].clone();

    let (evaluations, features) = (s.evaluations, s.doc.features.clone());
    let reply = run(&mut s, "drawing.inspect", json!({}));
    assert_eq!((s.evaluations, &s.doc.features), (evaluations, &features), "nothing evaluated");
    let lists = |sheet: &Value| ["notes", "tables", "images", "revision_tags"].map(|list| sheet[list].clone());
    let one = |objects: &[Value; 4]| objects.clone().map(|object| json!([object]));
    assert_eq!(lists(&reply["sheets"][0]), one(&here), "{reply}");
    assert_eq!(lists(&reply["sheets"][1]), one(&there), "{reply}");
    assert_eq!(lists(&reply["sheets"][2]), [json!([]), json!([]), json!([]), json!([])], "{reply}");
    let views = reply["sheets"][0]["views"].as_array().unwrap();
    let notes: Vec<&Value> = views.iter().map(|view| &view["notes"]).collect();
    assert_eq!(notes, [&json!([in_view]), &json!([]), &json!([])], "the note in the front view, under it only");

    // The same lists as the rule itself gives.
    let d = s.drawing("test").unwrap().clone();
    for (at, sheet) in d.sheets.iter().enumerate() {
        let on = d.on_sheet(sheet.id);
        let encoded = |list: &[u64]| json!(list.iter().map(|id| ids::encode(*id)).collect::<Vec<_>>());
        let listed = &reply["sheets"][at];
        assert_eq!(
            [&listed["tables"], &listed["images"], &listed["revision_tags"]],
            [&encoded(&on.tables), &encoded(&on.images), &encoded(&on.tags)]
        );
        let mut notes: Vec<Value> = listed["notes"].as_array().unwrap().clone();
        notes.extend(listed["views"].as_array().unwrap().iter().flat_map(|view| view["notes"].as_array().unwrap().clone()));
        notes.sort_by_key(|id| ids::decode(id.as_str().unwrap()));
        assert_eq!(json!(notes), encoded(&on.notes), "sheet {at}");
        let mut views: Vec<Value> = listed["views"].as_array().unwrap().iter().map(|view| view["id"].clone()).collect();
        views.sort_by_key(|id| ids::decode(id.as_str().unwrap()));
        assert_eq!(json!(views), encoded(&on.views), "sheet {at}: its views are the rule's");
    }

    // Deleted, each by its own command, is gone from its list. Selection delete takes notes; a table and a
    // revision tag have their own deletes; a placed image has no delete yet, so it stays listed.
    let [note, table, image, tag] = here;
    let drawing = ids::encode(d.id);
    let items: Vec<Value> = [&note, &in_view].map(|object| json!({"type": "drawing", "drawing": drawing, "object": object})).to_vec();
    run(&mut s, "selection.delete", json!({"items": items}));
    run(&mut s, "drawing.table.delete", json!({"table": table}));
    run(&mut s, "drawing.revision.tag.delete", json!({"tag": tag}));
    let reply = run(&mut s, "drawing.inspect", json!({}));
    assert_eq!(lists(&reply["sheets"][0]), [json!([]), json!([]), json!([image]), json!([])], "{reply}");
    assert_eq!(reply["sheets"][0]["views"][0]["notes"], json!([]));
    assert_eq!(lists(&reply["sheets"][1]), one(&there), "the other sheet's are kept");
}

/// Base views come in id order whatever order they were added in, and a view looking any other way
/// than a named one is `Custom`. A chain as deep as a chain goes is listed whole, top to bottom, and
/// the many-view drawing a record allows answers.
#[test]
fn inspect_lists_every_view_of_a_deep_or_large_drawing() {
    use solidcraft_doc::drawing::view::{MAX_VIEWS, ViewPlace};
    use solidcraft_doc::drawing_view::{Looking, ViewOverrides};
    let mut s = design();
    run(&mut s, "drawing.create", json!({}));
    let mut d = (**s.drawing("test").unwrap()).clone();
    let mut source = ids::IdSource::seeded(4894);
    let (sheet, drawn) = (d.sheets[0].id, d.sources[0].id);
    let scale = || ViewOverrides { scale: Some("1:1".parse().unwrap()), ..ViewOverrides::default() };
    let looking = ViewDirection::Looking(Looking { direction: [1.0, 1.0, -1.0], up: [0.0, 0.0, 1.0], origin: [0.0; 3] });
    let mut made = vec![d.allocate_id(&mut source).unwrap()];
    let kind = ViewKind::Base { source: drawn, view: looking };
    d.add_views([Arc::new(View {
        id: made[0],
        sheet,
        kind,
        place: ViewPlace::At([100.0, 100.0]),
        settings: scale(),
        reference_shown_percent: 0.0,
        label: None,
    })]);
    for _ in 1..MAX_VIEW_DEPTH {
        let id = d.allocate_id(&mut source).unwrap();
        let parent = *made.last().unwrap();
        let kind = ViewKind::Projected { parent, side: ProjectedSide::Below };
        d.add_views([Arc::new(View {
            id,
            sheet,
            kind,
            place: ViewPlace::At([100.0, 100.0]),
            settings: ViewOverrides::default(),
            reference_shown_percent: 0.0,
            label: None,
        })]);
        made.push(id);
    }
    d.validate().unwrap();
    s.doc_mut().put_drawing(Arc::new(d.clone()));
    let listed = run(&mut s, "drawing.inspect", json!({}))["sheets"][0]["views"].clone();
    let order: Vec<Value> = listed.as_array().unwrap().iter().map(|v| v["id"].clone()).collect();
    assert_eq!(order, made.iter().map(|id| json!(ids::encode(*id))).collect::<Vec<_>>());
    assert_eq!(listed[0]["label"], json!("Custom"));
    assert_eq!(listed[MAX_VIEW_DEPTH - 1]["label"], json!(format!("{}Custom", "Below ".repeat(MAX_VIEW_DEPTH - 1))));

    let base = |id: u64| View {
        id,
        sheet,
        kind: ViewKind::Base { source: drawn, view: ViewDirection::default() },
        place: ViewPlace::At([100.0, 100.0]),
        settings: scale(),
        reference_shown_percent: 0.0,
        label: None,
    };
    let more: Vec<Arc<View>> = (d.views.len()..MAX_VIEWS).map(|_| Arc::new(base(d.allocate_id(&mut source).unwrap()))).collect();
    let mut many = d.clone();
    many.add_views(more);
    many.validate().unwrap();
    let bases: Vec<Value> = many.views.iter().filter(|v| matches!(v.kind, ViewKind::Base { .. })).map(|v| json!(ids::encode(v.id))).collect();
    s.doc_mut().put_drawing(Arc::new(many));
    let listed = run(&mut s, "drawing.inspect", json!({}))["sheets"][0]["views"].clone();
    assert_eq!(listed.as_array().map(Vec::len), Some(MAX_VIEWS));
    let listed_bases: Vec<Value> = listed.as_array().unwrap().iter().filter(|v| v["kind"] == "base").map(|v| v["id"].clone()).collect();
    assert_eq!(listed_bases, bases, "base views in id order");
    let labelled = listed.as_array().unwrap().iter().filter(|v| v["label"] == json!("Front")).count();
    assert_eq!(labelled, MAX_VIEWS - MAX_VIEW_DEPTH);
}

/// The shared model-store port must not make a saved sheet sketch a model feature,
/// even when their numeric ids coincide. Drawing tools are not enabled by this port.
#[test]
fn sheet_records_do_not_redirect_the_shared_model_sketch_store() {
    use crate::cmd::sketch_store;
    use solidcore_record::Vec2;
    use solidcraft_doc::drawing::{Owner, SheetSketch};
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
                owner: Owner::Sheet(drawing.sheets[0].id),
                name: "Sheet sketch".into(),
                sketch: sheet_sketch.clone(),
            }));
        }
        Ok(())
    })
    .unwrap();
    let drawing = open_one(&s);
    let model = sketch_store::read(&s.doc, model_id).unwrap();
    assert_eq!(model.curves.len(), 1);
    assert!(model.resolve_point("sheet_point").is_none());
    assert!(sketch_store::read(&s.doc, separate).is_err());
    assert!(sketch_store::target(&s, &json!({"sketch": separate}), "test").is_err());
    // With the drawing still open and no sheet sketch entered, the sketch tools reach the model's
    // sketch of that id, never the sheet sketch sharing it.
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
    use solidcore_record::Vec2;

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

fn sheet_tile_context(drawing: u64, sheet: u64, focus: u64, owner_window: u64) -> crate::ViewContext {
    use solidcraft_views::ViewKind;
    let subject = format!("{}/{}", ids::encode(drawing), ids::encode(sheet));
    crate::ViewContext::new(
        vec![
            crate::ViewDescriptor { id: 1, kind: ViewKind::Model, window: 0, subject: None },
            crate::ViewDescriptor { id: 2, kind: ViewKind::Sheet, window: owner_window, subject: Some(subject.clone()) },
            crate::ViewDescriptor { id: 3, kind: ViewKind::Sheet, window: 0, subject: Some(subject) },
            crate::ViewDescriptor { id: 4, kind: ViewKind::Render, window: 0, subject: None },
        ],
        focus,
        vec![focus, if focus == 2 { 3 } else { 2 }, 1],
    )
    .unwrap()
}

fn hosted_sheet_editor(on_view: bool) -> (Session, Value) {
    let mut s = design();
    let drawing = run(&mut s, "drawing.create", json!({}));
    let drawing_id = ids::decode(drawing["id"].as_str().unwrap()).unwrap();
    let sheet = ids::decode(drawing["sheets"][0]["id"].as_str().unwrap()).unwrap();
    let owner = if on_view {
        run(&mut s, "drawing.view.base", json!({"sheet":ids::encode(sheet),"scale":"1:1"}))["view"].clone()
    } else {
        json!(ids::encode(sheet))
    };
    s.set_view_context(sheet_tile_context(drawing_id, sheet, 2, 0)).unwrap();
    let made = run(&mut s, "drawing.sketch.create", json!({"owner":owner,"view":2}));
    (s, made)
}

fn parked_sketch_exit_fixture(sheet_edit: bool, focused_model: bool) -> (Session, u64, u64, u64, u64) {
    let mut s = design();
    let owner = run(&mut s, "drawing.create", json!({"name":"Editing drawing"}));
    let other = run(&mut s, "drawing.create", json!({"name":"Focused drawing"}));
    let owner_id = ids::decode(owner["id"].as_str().unwrap()).unwrap();
    let other_id = ids::decode(other["id"].as_str().unwrap()).unwrap();
    let note = run(&mut s, "drawing.note.add", json!({"anchor":{"sheet":other["sheets"][0]["id"]},"at":[20,20],"content":"Selected source"}));
    let note = ids::decode(note["note"].as_str().unwrap()).unwrap();
    let context = |focused| {
        crate::ViewContext::new(
            vec![
                crate::ViewDescriptor { id: 1, kind: solidcraft_views::ViewKind::Model, window: 0, subject: None },
                crate::ViewDescriptor {
                    id: 2,
                    kind: solidcraft_views::ViewKind::Sheet,
                    window: 1,
                    subject: Some(format!("{}/{}", owner["id"].as_str().unwrap(), owner["sheets"][0]["id"].as_str().unwrap())),
                },
                crate::ViewDescriptor {
                    id: 3,
                    kind: solidcraft_views::ViewKind::Sheet,
                    window: 0,
                    subject: Some(format!("{}/{}", other["id"].as_str().unwrap(), other["sheets"][0]["id"].as_str().unwrap())),
                },
            ],
            focused,
            vec![focused, if focused == 2 { 3 } else { 2 }],
        )
        .unwrap()
    };
    s.open_drawing = Some(owner_id);
    s.model_focus = !sheet_edit;
    s.set_view_context(context(if sheet_edit { 2 } else { 1 })).unwrap();
    let made = if sheet_edit {
        run(&mut s, "drawing.sketch.create", json!({"owner":owner["sheets"][0]["id"],"view":2}))
    } else {
        run(&mut s, "sketch.create", json!({"plane":"XY","view":1}))
    };
    let sketch = if sheet_edit { ids::decode(made["sketch"].as_str().unwrap()).unwrap() } else { made["sketch"].as_u64().unwrap() };
    run(&mut s, "sketch.line", json!({"points":[[0,0],[10,0]]}));
    s.set_view_context(context(if focused_model { 1 } else { 3 })).unwrap();
    s.open_drawing = Some(other_id);
    s.model_focus = focused_model;
    s.selection = if focused_model {
        vec![crate::Sel::Feature { id: s.doc.features[0].id }]
    } else {
        vec![crate::Sel::Drawing { drawing: other_id, object: note }]
    };
    (s, owner_id, other_id, sketch, note)
}

#[test]
fn canonical_finish_and_cancel_target_the_parked_edit_and_keep_the_focused_receiver() {
    for sheet_edit in [true, false] {
        for command in ["sketch.finish", "sketch.cancel"] {
            for focused_model in [false, true] {
                if !sheet_edit && focused_model {
                    continue;
                }
                let (mut s, owner, other, sketch, _) = parked_sketch_exit_fixture(sheet_edit, focused_model);
                let focused = (s.open_drawing, s.model_focus, s.selection.clone(), s.camera, s.view_context.clone());
                let foreign = s.doc.drawing(other).unwrap().clone();
                let history = s.undo.len();
                let evaluations = s.evaluations;
                assert!(find_command(command).unwrap().allowed(&s).is_ok());
                let reply = run(&mut s, command, json!({}));
                assert_eq!(reply["sketch"], if sheet_edit { json!(ids::encode(sketch)) } else { json!(sketch) });
                assert!(s.active_sketch.is_none() && s.active_sheet_sketch.is_none());
                assert_eq!((s.open_drawing, s.model_focus, s.selection.clone(), s.camera, s.view_context.clone()), focused);
                assert!(Arc::ptr_eq(s.doc.drawing(other).unwrap(), &foreign));
                assert_eq!(s.undo.last().unwrap().part, if sheet_edit { Some(owner) } else { None });
                if command == "sketch.cancel" {
                    assert_eq!(s.undo.len(), history + 1);
                    s.open_drawing = if sheet_edit { Some(owner) } else { Some(other) };
                    s.model_focus = !sheet_edit;
                    run(&mut s, "edit.undo", json!({}));
                    if sheet_edit {
                        assert!(s.doc.drawing(owner).unwrap().sheet_sketch(sketch).is_some());
                    } else {
                        assert!(s.doc.feature(sketch).is_some());
                    }
                }
                if sheet_edit {
                    assert_eq!(s.evaluations, evaluations, "Sheet exits and undo do not rebuild the Model");
                }
            }
        }
    }
}

#[test]
fn parked_lifecycle_refusals_restore_the_receiver_and_authorize_the_actual_edit() {
    for level in crate::schema::ParamCheck::ALL {
        let (mut s, owner, _, _, _) = parked_sketch_exit_fixture(true, false);
        s.param_check = level;
        let before = (s.doc.to_json(), s.open_drawing, s.model_focus, s.selection.clone(), s.active_sheet_sketch.clone(), s.undo.len());
        assert!(s.execute("sketch.finish", &json!({"restore_marker":"invalid"})).is_err());
        assert_eq!((s.doc.to_json(), s.open_drawing, s.model_focus, s.selection.clone(), s.active_sheet_sketch.clone(), s.undo.len()), before);
        s.set_command_check(Some(Arc::new(move |command, _, session| {
            if command == "sketch.cancel" {
                assert_eq!(session.open_drawing, Some(owner));
                assert!(!session.model_focus);
                return Err("denied actual edit receiver".into());
            }
            Ok(())
        })));
        assert!(s.execute("sketch.cancel", &json!({})).is_err());
        assert_eq!((s.doc.to_json(), s.open_drawing, s.model_focus, s.selection.clone(), s.active_sheet_sketch.clone(), s.undo.len()), before);
    }
}

#[test]
fn starting_a_drawing_tool_preserves_the_other_focused_receivers_selected_source() {
    let (mut s, owner, other, sketch, note) = parked_sketch_exit_fixture(true, false);
    let selection = s.selection.clone();
    let event = s.prepare_edit_start("drawing.note.add", &Value::Null).unwrap().unwrap();
    assert_eq!(event.sketch, sketch);
    assert_eq!(s.selection, selection, "ending parked Sheet A does not erase Sheet B's selected source");
    let sheet = ids::encode(s.doc.drawing(other).unwrap().sheets[0].id);
    let reply = run(&mut s, "drawing.note.add", json!({"anchor":{"sheet":sheet},"at":[30,30],"content":"New note"}));
    assert_eq!(reply["finished_sketch"], event.name);
    assert_eq!(s.open_drawing, Some(other));
    assert_eq!(s.selection, vec![crate::Sel::Drawing { drawing: other, object: note }]);
    assert!(s.doc.drawing(owner).unwrap().sheet_sketch(sketch).is_some());
    assert_eq!(s.undo.last().unwrap().part, Some(other));
}

#[test]
fn canonical_parked_sketch_lifecycle_preserves_focus_selection_and_history_domain() {
    for sheet_edit in [false, true] {
        for command in ["sketch.finish", "sketch.cancel"] {
            let (mut s, drawing, _, sketch, _) = parked_sketch_exit_fixture(sheet_edit, sheet_edit);
            let focused = (s.open_drawing, s.model_focus, s.selection.clone(), s.camera, s.view_context.clone());
            let history = s.undo.len();
            assert!(find_command(command).unwrap().allowed(&s).is_ok());
            let reply = run(&mut s, command, json!({}));
            assert_eq!(reply["sketch"], if sheet_edit { json!(ids::encode(sketch)) } else { json!(sketch) });
            assert!(s.active_sketch.is_none() && s.active_sheet_sketch.is_none());
            assert_eq!((s.open_drawing, s.model_focus, s.selection.clone(), s.camera, s.view_context.clone()), focused);
            assert_eq!(s.undo.last().unwrap().part, if sheet_edit { Some(drawing) } else { None });
            if command == "sketch.cancel" {
                assert_eq!(s.undo.len(), history + 1);
                s.open_drawing = Some(drawing);
                s.model_focus = !sheet_edit;
                run(&mut s, "edit.undo", json!({}));
                if sheet_edit {
                    assert!(s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).is_some());
                } else {
                    assert!(s.doc.feature(sketch).is_some());
                }
            }
        }
    }
}

#[test]
fn parked_sketch_lifecycle_refusals_restore_focus_and_authorize_the_actual_edit() {
    for level in crate::schema::ParamCheck::ALL {
        for sheet_edit in [false, true] {
            let (mut s, drawing, other, _, _) = parked_sketch_exit_fixture(sheet_edit, sheet_edit);
            s.param_check = level;
            let before =
                (s.doc.to_json(), s.open_drawing, s.model_focus, s.selection.clone(), s.active_sketch, s.active_sheet_sketch.clone(), s.undo.len());
            assert!(s.execute("sketch.finish", &json!({"restore_marker":"invalid"})).is_err());
            assert_eq!(
                (s.doc.to_json(), s.open_drawing, s.model_focus, s.selection.clone(), s.active_sketch, s.active_sheet_sketch.clone(), s.undo.len()),
                before
            );
            s.set_command_check(Some(Arc::new(move |command, _, session| {
                if command == "sketch.cancel" {
                    assert_eq!(session.open_drawing, Some(if sheet_edit { drawing } else { other }));
                    assert_eq!(session.model_focus, !sheet_edit);
                    return Err("denied actual edit receiver".into());
                }
                Ok(())
            })));
            assert!(s.execute("sketch.cancel", &json!({})).is_err());
            assert_eq!(
                (s.doc.to_json(), s.open_drawing, s.model_focus, s.selection.clone(), s.active_sketch, s.active_sheet_sketch.clone(), s.undo.len()),
                before
            );
        }
    }
}

#[test]
fn automatic_parked_sketch_finish_keeps_the_focused_receivers_selection() {
    for sheet_edit in [false, true] {
        let (mut s, _, _, sketch, _) = parked_sketch_exit_fixture(sheet_edit, sheet_edit);
        let selected = s.selection.clone();
        let command = if sheet_edit { "solid.box" } else { "drawing.note.add" };
        let event = s.prepare_edit_start(command, &Value::Null).unwrap().unwrap();
        assert_eq!(event.sketch, sketch);
        assert_eq!(s.selection, selected);
        assert!(s.active_sketch.is_none() && s.active_sheet_sketch.is_none());
    }
}

#[test]
fn sheet_sketch_mode_owns_a_stable_tile_without_changing_the_model_camera() {
    for on_view in [false, true] {
        let (mut s, made) = hosted_sheet_editor(on_view);
        let entry = s.active_sheet_sketch.clone().unwrap();
        let sheet = entry.sheet(&s.doc).unwrap();
        assert_eq!(made["sketching_view"], 2);
        let schema = crate::schema::output_schema_for::<crate::schema::drawing_sketch::Reading>();
        assert!(crate::schema::tests::conforms(schema.as_value(), schema.as_value(), &made));
        assert_eq!(made.get("view").is_some(), on_view, "drawing geometry ownership is distinct from the editing tile");
        assert_eq!((s.editing_view(), s.sketching_view, s.active_sketch), (Some(2), None, None));
        let camera = s.camera;
        let evaluations = s.evaluations;
        run(&mut s, "sketch.line", json!({"points":[[10,10],[20,10]]}));
        s.set_view_context(sheet_tile_context(entry.drawing, sheet, 3, 7)).unwrap();
        assert_eq!(s.editing_view(), Some(2), "focus and moving to another window preserve ownership");
        assert!(s.prepare_view_close(3).unwrap().is_none());
        let event = s.prepare_view_close(2).unwrap().unwrap();
        assert_eq!((event.sketch, event.view), (entry.sketch, Some(2)));
        assert_eq!(event.domain, crate::SketchDomain::Sheet { drawing: entry.drawing, sheet });
        assert!(s.active_sheet_sketch.is_none());
        assert_eq!(s.camera, camera);
        assert_eq!(s.evaluations, evaluations, "sheet lifecycle never rebuilds the Model");
        assert_eq!(s.take_sketch_finish_event().unwrap(), event);
        assert!(s.take_sketch_finish_event().is_none());
        let reopened = run(&mut s, "drawing.sketch.edit", json!({"sketch":made["sketch"]}));
        assert_eq!(reopened["sketching_view"], 3);
        assert_eq!(s.editing_view(), Some(3));
        run(&mut s, "sketch.create", json!({"plane":"XY","view":1}));
        run(&mut s, "sketch.line", json!({"points":[[0,0],[10,0]]}));
        run(&mut s, "drawing.sketch.edit", json!({"sketch":made["sketch"],"view":2}));
        let sheet_exit = s.take_sketch_finish_event().unwrap();
        let model_exit = s.take_sketch_finish_event().unwrap();
        assert_eq!((sheet_exit.view, model_exit.view), (Some(3), Some(1)));
        assert!(matches!(sheet_exit.domain, crate::SketchDomain::Sheet { .. }));
        assert_eq!(model_exit.domain, crate::SketchDomain::Model);
        assert!(model_exit.sequence > sheet_exit.sequence);
        assert_eq!((s.editing_view(), s.active_sketch), (Some(2), None));
        assert_eq!(s.camera, camera, "neither Sheet transition changes the Model camera");
    }
}

#[test]
fn focusing_another_drawing_preserves_the_sheet_edit_and_its_receiver() {
    let mut s = design();
    let first = run(&mut s, "drawing.create", json!({"name":"First"}));
    let second = run(&mut s, "drawing.create", json!({"name":"Second"}));
    let first_id = ids::decode(first["id"].as_str().unwrap()).unwrap();
    let context = |focus| {
        crate::ViewContext::new(
            vec![
                crate::ViewDescriptor { id: 1, kind: solidcraft_views::ViewKind::Model, window: 0, subject: None },
                crate::ViewDescriptor {
                    id: 2,
                    kind: solidcraft_views::ViewKind::Sheet,
                    window: 0,
                    subject: Some(format!("{}/{}", first["id"].as_str().unwrap(), first["sheets"][0]["id"].as_str().unwrap())),
                },
                crate::ViewDescriptor {
                    id: 3,
                    kind: solidcraft_views::ViewKind::Sheet,
                    window: 1,
                    subject: Some(format!("{}/{}", second["id"].as_str().unwrap(), second["sheets"][0]["id"].as_str().unwrap())),
                },
            ],
            focus,
            vec![focus, if focus == 2 { 3 } else { 2 }, 1],
        )
        .unwrap()
    };
    run(&mut s, "drawing.open", json!({"drawing":first["id"]}));
    s.set_view_context(context(2)).unwrap();
    let made = run(&mut s, "drawing.sketch.create", json!({"owner":first["sheets"][0]["id"],"view":2}));
    run(&mut s, "sketch.line", json!({"points":[[0,0],[10,0]]}));
    let entry = s.active_sheet_sketch.clone();
    let evaluations = s.evaluations;
    s.set_view_context(context(3)).unwrap();
    run(&mut s, "drawing.open", json!({"drawing":second["id"]}));
    assert_eq!(s.active_sheet_sketch, entry);
    assert!(!s.sheet_sketching());
    refused(&mut s, "sketch.point", json!({"point":[5,7]}));
    assert_eq!(s.active_sheet_sketch, entry);
    s.set_view_context(context(2)).unwrap();
    run(&mut s, "drawing.open", json!({"drawing":first["id"]}));
    assert!(s.sheet_sketching());
    run(&mut s, "sketch.point", json!({"point":[5,7]}));
    s.set_view_context(context(3)).unwrap();
    run(&mut s, "drawing.open", json!({"drawing":second["id"]}));
    let event = s.prepare_view_close(2).unwrap().unwrap();
    assert_eq!(event.sketch, ids::decode(made["sketch"].as_str().unwrap()).unwrap());
    assert!(matches!(event.domain,crate::SketchDomain::Sheet {drawing,..} if drawing==first_id));
    assert_eq!(s.open_drawing, ids::decode(second["id"].as_str().unwrap()));
    assert!(s.active_sheet_sketch.is_none());
    assert_eq!(s.evaluations, evaluations);
}

#[test]
fn sheet_sketch_targets_reject_invalid_or_mismatched_views_at_every_check_level() {
    for level in crate::schema::ParamCheck::ALL {
        let (mut s, made) = hosted_sheet_editor(false);
        run(&mut s, "sketch.point", json!({"point":[5,7]}));
        run(&mut s, "sketch.finish", json!({}));
        s.param_check = level;
        let before = s.doc.to_json();
        let history = (s.undo.len(), s.redo.len(), s.ids.seed());
        for view in [json!(0), json!(-1), json!(1.5), json!("2"), json!(true), json!([]), json!(1), json!(4), json!(99), json!(u64::MAX)] {
            for command in ["drawing.sketch.create", "drawing.sketch.edit"] {
                let params = if command == "drawing.sketch.create" {
                    json!({"owner":made["sheet"],"view":view})
                } else {
                    json!({"sketch":made["sketch"],"view":view})
                };
                refused(&mut s, command, params);
                assert_eq!(s.doc.to_json(), before);
                assert_eq!((s.undo.len(), s.redo.len(), s.ids.seed()), history);
                assert!(s.active_sheet_sketch.is_none());
            }
        }
        let context = crate::ViewContext::new(
            vec![crate::ViewDescriptor { id: 2, kind: solidcraft_views::ViewKind::Sheet, window: 0, subject: Some("another sheet".into()) }],
            2,
            vec![2],
        )
        .unwrap();
        s.set_view_context(context).unwrap();
        refused(&mut s, "drawing.sketch.edit", json!({"sketch":made["sketch"],"view":2}));
        refused(&mut s, "drawing.sketch.edit", json!({"sketch":made["sketch"]}));
        assert_eq!(s.doc.to_json(), before);
    }
}

#[test]
fn headless_sheet_sketches_keep_no_tile_identity_and_finish_discards_only_empty_geometry() {
    let (mut s, drawing, empty, made) = sheet_editor();
    assert!(!s.view_context.is_hosted());
    assert_eq!(s.editing_view(), None);
    assert!(made.get("sketching_view").is_none());
    run(&mut s, "sketch.finish", json!({}));
    assert!(s.doc.drawing(drawing).unwrap().sheet_sketch(empty).is_none());
    let made = run(&mut s, "drawing.sketch.create", json!({"owner":made["sheet"]}));
    run(&mut s, "sketch.point", json!({"point":[5,7]}));
    let point = ids::decode(made["sketch"].as_str().unwrap()).unwrap();
    run(&mut s, "sketch.finish", json!({}));
    assert!(s.doc.drawing(drawing).unwrap().sheet_sketch(point).is_some());
    run(&mut s, "drawing.sketch.edit", json!({"sketch":made["sketch"]}));
    let event = s.prepare_edit_start("solid.box", &Value::Null).unwrap().unwrap();
    assert_eq!((event.sketch, event.view), (point, None), "headless exits never manufacture a view identity");
    assert!(matches!(event.domain, crate::SketchDomain::Sheet { .. }));
    let reply = run(&mut s, "solid.box", json!({"length":5,"width":5,"height":5,"corner":[60,0,0],"operation":"new"}));
    assert_eq!(reply["finished_sketch"], event.name);
}

#[test]
fn legacy_sheet_subjects_are_accepted_only_when_the_drawing_is_unambiguous() {
    let (mut s, drawing, _, made) = sheet_editor();
    run(&mut s, "sketch.point", json!({"point":[5,7]}));
    run(&mut s, "sketch.finish", json!({}));
    let legacy = crate::ViewContext::new(
        vec![crate::ViewDescriptor {
            id: 2,
            kind: solidcraft_views::ViewKind::Sheet,
            window: 0,
            subject: made["sheet"].as_str().map(str::to_string),
        }],
        2,
        vec![2],
    )
    .unwrap();
    s.set_view_context(legacy).unwrap();
    run(&mut s, "drawing.sketch.edit", json!({"sketch":made["sketch"],"view":2}));
    run(&mut s, "sketch.finish", json!({}));
    let mut duplicate = (**s.doc.drawing(drawing).unwrap()).clone();
    duplicate.id = drawing ^ (1_u64 << 62);
    duplicate.name = "Other drawing with the same local sheet ID".into();
    s.doc_mut().put_drawing(Arc::new(duplicate));
    refused(&mut s, "drawing.sketch.edit", json!({"sketch":made["sketch"],"view":2}));
    let sheet = ids::decode(made["sheet"].as_str().unwrap()).unwrap();
    s.set_view_context(sheet_tile_context(drawing, sheet, 2, 0)).unwrap();
    run(&mut s, "drawing.sketch.edit", json!({"sketch":made["sketch"],"view":2}));
    assert_eq!(s.editing_view(), Some(2));
}

#[test]
fn sheet_sketch_history_and_prepared_commands_never_restore_a_closed_tile() {
    let (mut s, made) = hosted_sheet_editor(false);
    let entry = s.active_sheet_sketch.clone().unwrap();
    run(&mut s, "sketch.line", json!({"points":[[10,10],[20,10]]}));
    let checkpoint = s.checkpoint();
    run(&mut s, "sketch.finish", json!({}));
    let commands = vec![("drawing.sketch.edit".into(), json!({"sketch":made["sketch"],"view":2}))];
    let prepared = s.scratch().compute_commands(&commands, None).unwrap();
    let mut raw = s.scratch();
    run(&mut raw, "drawing.sketch.edit", json!({"sketch":made["sketch"],"view":2}));
    let context =
        crate::ViewContext::new(vec![crate::ViewDescriptor { id: 1, kind: solidcraft_views::ViewKind::Model, window: 0, subject: None }], 1, vec![1])
            .unwrap();
    s.set_view_context(context.clone()).unwrap();
    let before = (s.doc.to_json(), s.undo.len(), s.revision);
    assert!(!prepared.is_current(&s));
    assert!(s.accept_commands(prepared).is_err());
    assert!(s.accept_computed(raw).is_err());
    assert_eq!((s.doc.to_json(), s.undo.len(), s.revision), before);
    run(&mut s, "edit.undo", json!({}));
    assert!(s.active_sheet_sketch.is_none());
    run(&mut s, "edit.redo", json!({}));
    assert!(s.active_sheet_sketch.is_none());
    s.restore_checkpoint(checkpoint).unwrap();
    assert!(s.doc.drawing(entry.drawing).unwrap().sheet_sketch(entry.sketch).is_some());
    assert!(s.active_sheet_sketch.is_none());
    assert_eq!(s.view_context, context);
}

#[test]
fn deleting_drawing_objects_finishes_sheet_mode_but_erasing_sketch_geometry_preserves_it() {
    let (mut s, made) = hosted_sheet_editor(true);
    let entry = s.active_sheet_sketch.clone().unwrap();
    let line = run(&mut s, "sketch.line", json!({"points":[[0,0],[10,0]]}))["curves"][0].clone();
    let evaluations = s.evaluations;
    run(&mut s, "selection.delete", json!({"items":[{"type":"sketch_curve","id":line}]}));
    assert_eq!(s.active_sheet_sketch.as_ref().unwrap().sketch, entry.sketch);
    assert!(s.take_sketch_finish_event().is_none());
    assert_eq!(s.evaluations, evaluations);
    run(&mut s, "sketch.point", json!({"point":[5,7]}));
    let reply = run(&mut s, "selection.delete", json!({"items":[{"type":"drawing","drawing":made["drawing"],"object":made["view"]}]}));
    assert!(reply.get("finished_sketch").is_some());
    let event = s.take_sketch_finish_event().unwrap();
    assert_eq!((event.sketch, event.view), (entry.sketch, Some(2)));
    assert!(s.active_sheet_sketch.is_none());
    assert!(s.doc.drawing(entry.drawing).unwrap().sheet_sketch(entry.sketch).is_none());
}

/// A 40 by 20 paper-mm rectangle with an optional 10 by 10 hole.
fn sheet_fill_fixture(hole: bool) -> (Session, u64, u64, Value, Value) {
    let (mut s, drawing, sketch, made) = sheet_editor();
    run(&mut s, "sketch.rectangle.two_point", json!({"p0":[15,10],"p1":[55,30]}));
    if hole {
        run(&mut s, "sketch.rectangle.two_point", json!({"p0":[25,15],"p1":[35,25]}));
    }
    run(&mut s, "sketch.finish", json!({}));
    let inspected = run(&mut s, "drawing.fill.profiles", json!({"sketch":made["sketch"]}));
    assert_eq!(inspected["fill_profiles"]["uncertified"], 0);
    let boundary = inspected["fill_profiles"]["boundaries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|boundary| boundary["holes"].as_array().unwrap().len() == usize::from(hole))
        .unwrap()
        .clone();
    (s, drawing, sketch, made, boundary)
}

#[test]
fn sheet_fills_keep_complete_holes_paper_units_and_drawing_history() {
    use solidcraft_geom::paper::{Item, ObjectKind};
    let (mut s, drawing, _, made, boundary) = sheet_fill_fixture(true);
    let model = s.model.state();
    let features = s.doc.features.clone();
    let (evaluations, undo) = (s.evaluations, s.undo.len());
    let result = run(&mut s, "drawing.sketch.fill", json!({"sketch":made["sketch"],"boundary":boundary,"paint":{"kind":"solid"}}));
    let fill = ids::decode(result["fills"][0]["fill"].as_str().unwrap()).unwrap();
    assert_eq!(result["fills"][0]["boundary"], boundary);
    assert_eq!(result["fills"][0]["status"], "current");
    assert_eq!((s.undo.len(), s.undo.last().unwrap().part), (undo + 1, Some(drawing)));
    let owner = open_one(&s);
    let scene = crate::drawing_scene::build(&s, &owner, owner.sheets[0].id).unwrap();
    let group = scene.scene.groups.iter().find(|group| group.object == fill).unwrap();
    assert_eq!(group.kind, ObjectKind::Fill);
    let [Item::Fill { region }] = group.items.as_slice() else { panic!("not one solid fill: {group:?}") };
    assert_eq!(region.holes.len(), 1);
    assert_eq!(region.outer.signed_area().abs() - region.holes.iter().map(|hole| hole.signed_area().abs()).sum::<f64>(), 700.0);
    let before = s.doc.clone();
    run(&mut s, "drawing.fill.edit", json!({"fill":ids::encode(fill),"paint":{"kind":"solid"}}));
    assert!(Arc::ptr_eq(&before, &s.doc), "unchanged paint keeps the saved drawing");
    assert_eq!(s.undo.len(), undo + 1);
    run(&mut s, "edit.undo", json!({}));
    assert!(open_one(&s).fills.is_empty());
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(open_one(&s).fills[0].id, fill);
    assert!(Arc::ptr_eq(&model, &s.model.state()));
    assert_eq!((s.evaluations, &s.doc.features), (evaluations, &features));
}

#[test]
fn sheet_hatch_dots_are_eighty_real_disks_and_never_zero_length_strokes() {
    use solidcraft_geom::paper::Item;
    let (mut s, _, _, made, boundary) = sheet_fill_fixture(true);
    let imported = run(&mut s, "drawing.pattern.import", json!({"kind":"pat","source":"*DOTS,Original analytic dots\n0,0,0,0,2,0,-4\n\n"}));
    let pattern = imported["patterns"][0]["id"].clone();
    let result = run(
        &mut s,
        "drawing.sketch.fill",
        json!({"sketch":made["sketch"],"boundary":boundary,"paint":{"kind":"hatch","pattern":pattern,"pen_width_mm":0.25}}),
    );
    let fill = ids::decode(result["fills"][0]["fill"].as_str().unwrap()).unwrap();
    let owner = open_one(&s);
    let built = crate::drawing_scene::build(&s, &owner, owner.sheets[0].id).unwrap();
    let group = built.scene.groups.iter().find(|group| group.object == fill).unwrap();
    // Nine strict-interior y rows, ten x marks each, less ten inside the hole.
    assert_eq!(group.items.len(), 80);
    for item in &group.items {
        let Item::Fill { region } = item else { panic!("dot became a stroke: {item:?}") };
        assert!(region.holes.is_empty());
        let area = region.outer.signed_area().abs();
        assert!((area - std::f64::consts::PI * 0.125_f64.powi(2)).abs() < 1e-10, "dot area {area}");
    }
    let prepared = built.scene.prepare().unwrap();
    let (svg, _) = solidcraft_io::paper::paper_svg(&prepared, &solidcraft_geom::paper::NoPictures, 300.0).unwrap();
    assert!(String::from_utf8(svg).unwrap().contains("fill-rule=\"evenodd\""));
}

#[test]
fn sheet_pattern_snapshots_preserve_duplicates_phase_units_and_references() {
    let (mut s, _, _, made, boundary) = sheet_fill_fixture(false);
    let source = "*PHASE,Original\n0,1,2,-1,2,1,-1,0,-2\n\n*PHASE,Separate definition\n90,0,0,0,3\n\n";
    let imported = run(&mut s, "drawing.pattern.import", json!({"kind":"pat","source":source,"units":"inches","scale":0.5}));
    assert_eq!(imported["patterns"].as_array().unwrap().len(), 2);
    let first = imported["patterns"][0]["id"].clone();
    let second = imported["patterns"][1]["id"].clone();
    assert_ne!(first, second);
    let family = &imported["patterns"][0]["families"][0];
    assert_eq!(family["origin"], json!([12.7, 25.4]));
    assert_eq!(family["delta_x"], -12.7);
    assert_eq!(family["dashes"], json!([12.7, -12.7, 0.0, -25.4]));
    run(
        &mut s,
        "drawing.sketch.fill",
        json!({"sketch":made["sketch"],"boundary":boundary,"paint":{"kind":"hatch","pattern":first,"pen_width_mm":0.25}}),
    );
    let before = s.doc.clone();
    assert!(refused(&mut s, "drawing.pattern.delete", json!({"pattern":first})).contains("referenced"));
    assert!(Arc::ptr_eq(&before, &s.doc));
    run(&mut s, "drawing.pattern.delete", json!({"pattern":second}));
    assert_eq!(run(&mut s, "drawing.pattern.inspect", json!({"pattern":first}))["patterns"][0], imported["patterns"][0]);
}

#[test]
fn sheet_fill_loss_retains_the_last_region_reopens_and_refuses_export_atomically() {
    let (mut s, drawing, _, made, boundary) = sheet_fill_fixture(true);
    let result = run(&mut s, "drawing.sketch.fill", json!({"sketch":made["sketch"],"boundary":boundary,"paint":{"kind":"solid"}}));
    let fill = result["fills"][0]["fill"].clone();
    let saved_region = open_one(&s).fills[0].last_region.clone();
    run(&mut s, "drawing.sketch.edit", json!({"sketch":made["sketch"]}));
    run(&mut s, "sketch.delete", json!({"entities":[boundary["outer"][0]]}));
    run(&mut s, "sketch.finish", json!({}));
    assert_eq!(run(&mut s, "drawing.fill.inspect", json!({"fill":fill}))["fills"][0]["status"], "lost_boundary");
    assert_eq!(open_one(&s).fills[0].last_region, saved_region);
    let built = run(&mut s, "drawing.scene", json!({"sheet":made["sheet"]}));
    assert_eq!(built["fills"][0]["status"], "lost_boundary");
    let folder = Folder::new("drawing-fill-lost");
    let path = folder.path("existing.svg");
    std::fs::write(&path, b"retain previous output").unwrap();
    let why = refused(&mut s, "drawing.export", json!({"sheet":made["sheet"],"path":path}));
    assert!(why.contains(fill.as_str().unwrap()) && why.contains("lost boundary"), "{why}");
    assert_eq!(std::fs::read(&path).unwrap(), b"retain previous output");
    let absent = folder.path("absent.svg");
    refused(&mut s, "drawing.export", json!({"sheet":made["sheet"],"path":absent}));
    assert!(!std::path::Path::new(&absent).exists());
    let mut reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    run(&mut reopened, "drawing.open", json!({"drawing":ids::encode(drawing)}));
    assert_eq!(run(&mut reopened, "drawing.fill.inspect", json!({"fill":fill}))["fills"][0]["status"], "lost_boundary");
    assert_eq!(open_one(&reopened).fills[0].last_region, saved_region);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(run(&mut s, "drawing.fill.inspect", json!({"fill":fill}))["fills"][0]["status"], "current");
}

#[test]
fn sheet_fill_refusals_are_atomic_and_retired_ids_are_not_reused() {
    let (mut s, _, _, made, boundary) = sheet_fill_fixture(true);
    for request in [
        json!({"sketch":made["sheet"],"boundary":boundary,"paint":{"kind":"solid"}}),
        json!({"sketch":made["sketch"],"boundary":{"outer":boundary["outer"],"holes":[]},"paint":{"kind":"solid"}}),
        json!({"sketch":made["sketch"],"boundary":boundary,"paint":{"kind":"hatch","pattern":made["sheet"],"pen_width_mm":0.25}}),
    ] {
        let before = s.doc.clone();
        let history = (s.undo.len(), s.redo.len(), s.evaluations);
        refused(&mut s, "drawing.sketch.fill", request);
        assert!(Arc::ptr_eq(&before, &s.doc));
        assert_eq!((s.undo.len(), s.redo.len(), s.evaluations), history);
    }
    let request = json!({"sketch":made["sketch"],"boundary":boundary,"paint":{"kind":"solid"}});
    let first = run(&mut s, "drawing.sketch.fill", request.clone())["fills"][0]["fill"].clone();
    run(&mut s, "edit.undo", json!({}));
    let second = run(&mut s, "drawing.sketch.fill", request)["fills"][0]["fill"].clone();
    assert_ne!(first, second);
}

#[test]
fn sheet_fill_commands_have_checked_schemas_and_hostile_requests_do_not_publish() {
    use crate::schema::drawing_fill::{Patterns, Readings};
    use crate::schema::tests::conforms;
    use crate::schema::{ParamCheck, output_schema_for};
    let commands = [
        "drawing.sketch.fill",
        "drawing.fill.edit",
        "drawing.fill.delete",
        "drawing.fill.inspect",
        "drawing.fill.profiles",
        "drawing.pattern.import",
        "drawing.pattern.inspect",
        "drawing.pattern.delete",
    ];
    let (s, _, _, _, _) = sheet_fill_fixture(false);
    for command in commands {
        let spec = find_command(command).unwrap();
        assert!(spec.contract.is_some() && spec.output.is_some(), "{command}");
        for value in crate::tests::hostile_values() {
            for request in [value.clone(), json!({"fill":value}), json!({"pattern":value}), json!({"boundary":value}), json!({"kind":value})] {
                let mut copy = s.scratch();
                let before = copy.doc.clone();
                let history = (copy.undo.len(), copy.redo.len(), copy.evaluations);
                assert!(!matches!(copy.execute(command, &request), Err(crate::EngineError::Internal(..))), "{command}: {request}");
                assert!(Arc::ptr_eq(&before, &copy.doc), "{command} published hostile request {request}");
                assert_eq!((copy.undo.len(), copy.redo.len(), copy.evaluations), history);
            }
        }
    }
    for level in [ParamCheck::Off, ParamCheck::Strict] {
        let (mut s, _, _, made, boundary) = sheet_fill_fixture(false);
        s.param_check = level;
        let patterns = run(&mut s, "drawing.pattern.import", json!({"kind":"builtin","pattern":"lines_45"}));
        let schema = output_schema_for::<Patterns>();
        assert!(conforms(schema.as_value(), schema.as_value(), &patterns), "{patterns}");
        let result = run(&mut s, "drawing.sketch.fill", json!({"sketch":made["sketch"],"boundary":boundary,"paint":{"kind":"solid"}}));
        let schema = output_schema_for::<Readings>();
        assert!(conforms(schema.as_value(), schema.as_value(), &result), "{result}");
        let fill = result["fills"][0]["fill"].clone();
        assert_eq!(run(&mut s, "drawing.fill.inspect", json!({"fill":fill})), result);
        let count = s.undo.len();
        run(&mut s, "drawing.fill.delete", json!({"fill":fill}));
        assert!(open_one(&s).fills.is_empty());
        assert_eq!(s.undo.len(), count + 1);
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(run(&mut s, "drawing.fill.inspect", json!({"fill":fill})), result);
    }
}

#[test]
fn sheet_fill_profiles_refuse_equation_metadata_without_disabling_sketch_inspection() {
    let (mut s, _, sketch, made, boundary) = sheet_fill_fixture(false);
    let mut candidate = (*open_one(&s)).clone();
    let record = candidate.sketches.iter_mut().find(|record| record.id == sketch).unwrap();
    let record = Arc::make_mut(record);
    record.sketch.equations.push(solidcraft_sketch::EquationCurve {
        curve: record.sketch.curves[0].id.clone(),
        x: None,
        y: "x".into(),
        start: "0".into(),
        end: "1".into(),
        variable_unit: solidcraft_sketch::VariableUnit::Number,
        tolerance: 0.01,
    });
    let before = s.doc.clone();
    let history = (s.undo.len(), s.redo.len(), s.evaluations);
    let why = s
        .edit_drawing("test", |drawing| {
            *drawing = candidate.clone();
            Ok(())
        })
        .unwrap_err()
        .to_string();
    assert!(why.contains("equation"), "{why}");
    assert!(Arc::ptr_eq(&before, &s.doc));
    assert_eq!((s.undo.len(), s.redo.len(), s.evaluations), history);
    // Refused metadata never replaces the admitted sketch, which remains inspectable.
    let reading = run(&mut s, "drawing.sketch.inspect", json!({"sketch":made["sketch"]}));
    assert_eq!(reading["curves"].as_array().unwrap().len(), 4);
    assert!(Arc::ptr_eq(&before, &s.doc));
    assert_eq!((s.undo.len(), s.redo.len(), s.evaluations), history);

    // Deliberately corrupt only a scratch session to exercise defensive fill commands
    // beyond public admission. Such a record cannot be edited in, loaded or saved.
    let mut malformed = s.scratch();
    malformed.doc_mut().put_drawing(Arc::new(candidate));
    let malformed_before = malformed.doc.clone();
    let malformed_history = (malformed.undo.len(), malformed.redo.len(), malformed.evaluations);
    for (command, request) in [
        ("drawing.fill.profiles", json!({"sketch":made["sketch"]})),
        ("drawing.sketch.fill", json!({"sketch":made["sketch"],"boundary":boundary,"paint":{"kind":"solid"}})),
    ] {
        let why = refused(&mut malformed, command, request);
        assert!(why.contains("equation"), "{command}: {why}");
        assert!(Arc::ptr_eq(&malformed_before, &malformed.doc));
        assert_eq!((malformed.undo.len(), malformed.redo.len(), malformed.evaluations), malformed_history);
    }
    assert!(Arc::ptr_eq(&before, &s.doc));
    assert_eq!((s.undo.len(), s.redo.len(), s.evaluations), history);
}

#[test]
fn sheet_hatch_dot_precision_refusal_never_publishes_the_candidate_fill() {
    let (mut s, _, _, made, boundary) = sheet_fill_fixture(false);
    let pattern = run(&mut s, "drawing.pattern.import", json!({"kind":"pat","source":"*DOTS,Original precision fixture\n0,0,0,0,2,0,-4\n\n"}));
    let before = s.doc.clone();
    let history = (s.undo.len(), s.redo.len(), s.evaluations);
    let why = refused(
        &mut s,
        "drawing.sketch.fill",
        json!({"sketch":made["sketch"],"boundary":boundary,
        "paint":{"kind":"hatch","pattern":pattern["patterns"][0]["id"],"pen_width_mm":1e-14}}),
    );
    assert!(why.contains("dot") || why.contains("disk"), "{why}");
    assert!(Arc::ptr_eq(&before, &s.doc));
    assert_eq!((s.undo.len(), s.redo.len(), s.evaluations), history);
    assert!(open_one(&s).fills.is_empty());
}

#[test]
fn sheet_editor_sketch_ids_follow_the_sessions_seed_and_survive_refusal() {
    let made = |seed: u64, fail: bool| {
        let mut s = Session::default();
        s.seed_ids(seed);
        run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 10}));
        let drawing = run(&mut s, "drawing.create", json!({}));
        let owner = drawing["sheets"][0]["id"].clone();
        if fail {
            refused(&mut s, "drawing.sketch.create", json!({"owner": owner, "name": " "}));
        }
        run(&mut s, "drawing.sketch.create", json!({"owner": owner}))["sketch"].clone()
    };
    assert_eq!(made(7, false), made(7, false));
    assert_eq!(made(7, false), made(7, true), "a refused create consumes no id");
    assert_ne!(made(7, false), made(8, false));
}

#[test]
fn sheet_editor_computed_creation_never_reuses_a_retired_sketch_id() {
    for checked_commands in [false, true] {
        let mut s = Session::default();
        s.seed_ids(7082);
        run(&mut s, "solid.box", json!({"length":40,"width":30,"height":10}));
        let drawing = run(&mut s, "drawing.create", json!({}));
        let params = json!({"owner":drawing["sheets"][0]["id"]});
        let mut discarded = s.scratch();
        let discarded_id = run(&mut discarded, "drawing.sketch.create", params.clone())["sketch"].clone();
        drop(discarded);
        let made = if checked_commands {
            let prepared = s.scratch().compute_commands(&[("drawing.sketch.create".into(), params.clone())], None).unwrap();
            s.accept_commands(prepared).unwrap().remove(0).1
        } else {
            let mut computed = s.scratch();
            let made = run(&mut computed, "drawing.sketch.create", params.clone());
            s.accept_computed(computed).unwrap();
            made
        };
        assert_eq!(made["sketch"], discarded_id, "discarding a scratch consumes no id");
        run(&mut s, "sketch.point", json!({"point":[5,7]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "edit.undo", json!({}));
        run(&mut s, "edit.undo", json!({}));
        assert!(s.drawing("test").unwrap().sketches.is_empty());
        run(&mut s, "drawing.set", json!({"name":"After retirement"}));
        assert!(s.redo.is_empty(), "the old record is no longer retained by redo");
        let next = run(&mut s, "drawing.sketch.create", params);
        assert_ne!(next["sketch"], made["sketch"], "computed adoption must advance the id source ({checked_commands})");
    }
}

#[test]
fn sheet_editor_owner_deletion_requires_finish_and_explicit_contents() {
    let (mut s, drawing, sketch, _) = sheet_editor();
    run(&mut s, "sketch.point", json!({"point":[5,7]}));
    let first = ids::encode(s.doc.drawing(drawing).unwrap().sheets[0].id);
    let added = run(&mut s, "drawing.sheet.add", json!({"name":"Other"}));
    let before = s.doc.clone();
    let editing = s.active_sheet_sketch.clone();
    let steps = s.undo.len();
    let why = refused(&mut s, "drawing.sheet.delete", json!({"sheet":first,"contents":"delete"}));
    assert!(why.contains("finish or cancel"), "{why}");
    assert!(Arc::ptr_eq(&s.doc, &before));
    assert_eq!(s.active_sheet_sketch, editing);
    assert_eq!(s.undo.len(), steps);
    run(&mut s, "sketch.finish", json!({}));
    let why = refused(&mut s, "drawing.sheet.delete", json!({"sheet":first}));
    assert!(why.contains("1 sheet sketch") && why.contains(&ids::encode(sketch)), "{why}");
    run(&mut s, "drawing.sheet.delete", json!({"sheet":first,"contents":"delete"}));
    let remaining = s.doc.drawing(drawing).unwrap();
    assert!(remaining.sketches.is_empty());
    assert_eq!(json!(ids::encode(remaining.sheets[0].id)), added["sheet"]);
    run(&mut s, "edit.undo", json!({}));
    assert!(s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).is_some());
    run(&mut s, "edit.redo", json!({}));
    assert!(s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).is_none());
}

#[test]
fn sheet_editor_existing_tools_make_paper_geometry_without_evaluating_the_model() {
    use solidcore_record::Vec2;
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
        ("drawing.close", json!({})),
        ("drawing.open", json!({"drawing": made["drawing"]})),
        ("drawing.delete", json!({"drawing": made["drawing"]})),
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
fn sheet_editor_new_edits_finish_the_previous_receiver_even_when_refused() {
    for command in ["sketch.create", "drawing.sketch.create"] {
        let (mut s, drawing, sketch, made) = sheet_editor();
        run(&mut s, "sketch.line", json!({"points":[[10,10],[20,10]]}));
        let evaluations = s.evaluations;
        if command == "sketch.create" {
            // A model sketch ends the sheet sketch first, then opens.
            run(&mut s, command, json!({"plane":"XY"}));
            assert!(s.active_sheet_sketch.is_none());
            assert!(s.active_sketch.is_some());
        } else {
            let next = run(&mut s, command, json!({"owner":made["sheet"]}));
            assert_ne!(next["sketch"], made["sketch"]);
            assert_ne!(s.active_sheet_sketch.as_ref().unwrap().sketch, sketch);
        }
        let event = s.take_sketch_finish_event().unwrap();
        assert_eq!(event.sketch, sketch);
        assert!(matches!(event.domain, crate::SketchDomain::Sheet { drawing: owner, .. } if owner == drawing));
        assert!(s.take_sketch_finish_event().is_none());
        if command != "sketch.create" {
            assert_eq!(s.evaluations, evaluations);
        }
    }
}

#[test]
fn a_sheet_edit_finishes_before_a_model_feature_or_sketch_starts() {
    let (mut s, drawing, sketch, _) = sheet_editor();
    run(&mut s, "sketch.line", json!({"points":[[10,10],[20,10]]}));
    let features = s.doc.features.len();
    let reply = run(&mut s, "solid.box", json!({"length":5,"width":5,"height":5,"corner":[60,0,0],"operation":"new"}));
    assert_eq!(reply["finished_sketch"], "Detail");
    assert_eq!(s.doc.features.len(), features + 1);
    assert!(s.active_sheet_sketch.is_none() && s.active_sketch.is_none());
    assert_eq!(s.open_drawing, Some(drawing));
    assert_eq!(s.undo.last().unwrap().part, None);
    assert_eq!(s.take_sketch_finish_event().unwrap().sketch, sketch);
    let made = run(&mut s, "sketch.create", json!({"plane":"XY"}));
    run(&mut s, "sketch.point", json!({"point":[3,4]}));
    assert_eq!(s.active_sketch, made["sketch"].as_u64());
    assert!(s.active_sheet_sketch.is_none());
    assert_eq!(s.undo.last().unwrap().part, None, "model sketch geometry belongs to model history with the drawing retained");
}

#[test]
fn model_focus_routes_history_without_closing_the_drawing() {
    let mut s = design();
    run(&mut s, "drawing.create", json!({"name":"Paper"}));
    let open = s.open_drawing;
    let features = s.doc.features.len();
    s.model_focus = true;
    run(&mut s, "solid.box", json!({"length":5,"width":5,"height":5,"corner":[60,0,0],"operation":"new"}));
    s.model_focus = false;
    run(&mut s, "drawing.set", json!({"name":"Revised paper"}));
    s.model_focus = true;
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.features.len(), features);
    assert_eq!(s.drawing("test").unwrap().name, "Revised paper");
    assert_eq!(s.open_drawing, open);
    assert!(s.model_focus);
    s.model_focus = false;
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.drawing("test").unwrap().name, "Paper");
    assert_eq!(s.doc.features.len(), features);
    assert_eq!(s.open_drawing, open);
}

#[test]
fn model_history_never_reopens_an_editor_beside_an_active_sheet_sketch() {
    let mut s = design();
    let drawing = run(&mut s, "drawing.create", json!({}));
    s.model_focus = true;
    run(&mut s, "sketch.create", json!({"plane":"XY"}));
    run(&mut s, "sketch.line", json!({"points":[[0,0],[10,0]]}));
    run(&mut s, "sketch.finish", json!({}));
    s.model_focus = false;
    run(&mut s, "drawing.sketch.create", json!({"owner":drawing["sheets"][0]["id"]}));
    run(&mut s, "sketch.line", json!({"points":[[10,10],[20,10]]}));
    let sheet = s.active_sheet_sketch.clone();
    s.model_focus = true;
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.active_sheet_sketch, sheet);
    assert!(s.active_sketch.is_none() && s.sketching_view.is_none());
    assert!(s.take_sketch_finish_event().is_none(), "history does not invent a new finish event");
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
    run(&mut s, "sketch.point", json!({"point":[5,7]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "drawing.sketch.edit", json!({"sketch": made["sketch"]}));
    run(&mut s, "sketch.cancel", json!({}));
}

/// A sketch can belong to a view (#5425). `drawing.sketch.create` makes one from the view's id, and
/// it is edited, finished and cancelled as a sheet's is, its replies naming its view as well as its
/// sheet. While it is being edited, neither its view nor its view's sheet can be deleted. Copying
/// the view to a sheet copies it, owned by the copy; deleting its view takes it, as it takes the
/// view's notes.
#[test]
fn a_view_sketch_is_made_edited_copied_and_deleted_with_its_view() {
    use solidcraft_doc::drawing::Owner;
    let (mut s, d, on_sheet, made) = sheet_editor();
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [40, 0]]}));
    run(&mut s, "sketch.finish", json!({}));
    let sheet = made["sheet"].clone();
    let other = run(&mut s, "drawing.sheet.add", json!({"name": "Other"}))["sheet"].clone();
    let view = run(&mut s, "drawing.view.base", json!({"sheet": sheet, "scale": "1:1"}))["view"].clone();
    let view_id = ids::decode(view.as_str().unwrap()).unwrap();
    let before = open_one(&s);
    let e = refused(&mut s, "drawing.sketch.create", json!({"owner": ids::encode(on_sheet)}));
    assert!(e.contains("no such sheet or view"), "{e}");
    assert!(Arc::ptr_eq(&before, &open_one(&s)) && s.active_sheet_sketch.is_none(), "a refused owner changes nothing");

    let undo = s.undo.len();
    let made = run(&mut s, "drawing.sketch.create", json!({"owner": view, "name": "On the view"}));
    assert_eq!((&made["sheet"], &made["view"], made["editing"].as_bool()), (&sheet, &view, Some(true)));
    assert_eq!(s.undo.len(), undo + 1, "one undo step");
    let on_view = ids::decode(made["sketch"].as_str().unwrap()).unwrap();
    assert_eq!(open_one(&s).sheet_sketch(on_view).unwrap().owner, Owner::View(view_id));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [30, 0]]}));
    let item = json!({"items": [{"type": "drawing", "drawing": ids::encode(d), "object": view}]});
    let mut preview = item.clone();
    preview["dry_run"] = json!(true);
    let e = refused(&mut s, "selection.delete", preview);
    assert!(e.contains("finish or cancel"), "{e}");
    let e = refused(&mut s, "drawing.sheet.delete", json!({"sheet": sheet, "contents": "delete"}));
    assert!(e.contains("finish or cancel"), "{e}");
    let finished = run(&mut s, "sketch.finish", json!({}));
    assert_eq!((&finished["view"], finished["editing"].as_bool()), (&view, Some(false)));

    let kept = open_one(&s).sheet_sketch(on_view).unwrap().clone();
    let entered = run(&mut s, "drawing.sketch.edit", json!({"sketch": made["sketch"]}));
    assert_eq!((&entered["view"], entered["editing"].as_bool()), (&view, Some(true)));
    run(&mut s, "sketch.line", json!({"points": [[0, 10], [30, 10]]}));
    let cancelled = run(&mut s, "sketch.cancel", json!({}));
    assert_eq!((&cancelled["sheet"], &cancelled["view"], cancelled["removed"].as_bool()), (&sheet, &view, Some(false)));
    assert!(Arc::ptr_eq(open_one(&s).sheet_sketch(on_view).unwrap(), &kept), "cancel restores it");
    let entered = run(&mut s, "drawing.sketch.edit", json!({"sketch": ids::encode(on_sheet)}));
    assert!(entered.get("view").is_none(), "a sheet's sketch names no view: {entered}");
    run(&mut s, "sketch.finish", json!({}));

    let copy = run(&mut s, "drawing.view.copy_to_sheet", json!({"view": view, "sheet": other}))["view"].clone();
    let copy = ids::decode(copy.as_str().unwrap()).unwrap();
    let after = open_one(&s);
    let copies: Vec<_> = after.sketches.iter().filter(|record| record.owner == Owner::View(copy)).collect();
    assert_eq!(copies.len(), 1, "the copy takes its view's sketch");
    assert!(copies[0].id != on_view && copies[0].sketch == kept.sketch && copies[0].name == kept.name);
    assert!(Arc::ptr_eq(after.sheet_sketch(on_view).unwrap(), &kept), "the original is unchanged");

    let deleted = run(&mut s, "selection.delete", item);
    assert_eq!(
        (&deleted["views"], &deleted["sketches"], &deleted["fills"]),
        (&json!([view]), &json!([ids::encode(on_view)]), &json!([])),
        "{deleted}"
    );
    let after = open_one(&s);
    assert!(after.sheet_sketch(on_view).is_none(), "the view's sketch goes with it");
    assert!(after.sheet_sketch(on_sheet).is_some() && after.sheet_sketch(copies[0].id).is_some(), "the others stay");
}

#[test]
fn sheet_editor_same_numeric_id_in_two_drawings_and_model_never_retargets_geometry() {
    use solidcraft_doc::drawing::{Owner, SheetSketch};
    use solidcraft_sketch::Sketch;
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Model"}));
    let id = s.active_sketch.unwrap();
    run(&mut s, "sketch.line", json!({"points": [[0,0],[40,0]]}));
    run(&mut s, "sketch.finish", json!({}));
    let model = s.doc.sketch(id).unwrap().clone();
    run(&mut s, "drawing.create", json!({"name": "First"}));
    s.edit_drawing("test", |drawing| {
        drawing.sketches.push(Arc::new(SheetSketch { id, owner: Owner::Sheet(drawing.sheets[0].id), name: "Same id".into(), sketch: Sketch::new() }));
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
    // A refusal on the sheet (model commands are no longer refused there, so a drawing switch).
    refused(&mut s, "drawing.close", json!({}));
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.redo", json!({}));
    run(&mut s, "sketch.finish", json!({}));
    assert!(s.model.is_stale());
    assert!(Arc::ptr_eq(&model, &s.model.state()));
    assert_eq!(s.evaluations, count);
    assert!(s.active_sketch.is_none(), "entering the sheet sketch finished the model edit");
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
    other.sketch.add_point(solidcore_record::Vec2::new(4.0, 5.0), Some("invalid_point")).unwrap();
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
    use solidcore_record::Vec2;
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
            assert!(s.active_sketch.is_none());
            assert_eq!((s.undo.len(), s.redo.len(), s.evaluations), history);
        }
    }
}

#[test]
fn sheet_next_circles_keep_analytic_centres_radii_and_paper_units() {
    use solidcore_record::Vec2;
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
    use solidcore_record::Vec2;
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
    use solidcore_record::Vec2;
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
    s.accept_computed(computed).unwrap();
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
    s.accept_computed(closed).unwrap();
    assert!(s.open_drawing.is_none() && s.active_sheet_sketch.is_none());
    run(&mut s, "edit.restore_checkpoint", json!({"checkpoint":checkpoint}));
    assert_eq!(s.open_drawing, Some(ids::decode(a["id"].as_str().unwrap()).unwrap()));
    assert!(s.active_sheet_sketch.is_none());
    assert_eq!(s.doc.to_json(), before.to_json());
}

#[test]
fn sheet_next_curved_point_expressions_use_paper_defaults_and_keep_source_read_only() {
    use solidcore_record::Vec2;
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
    use solidcore_record::Vec2;
    use solidcraft_sketch::{ConstraintKind, CurveKind};
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
        // Current circumscribed polygons retain regularity with an incircle and
        // a concentric construction circle through the corners (#7395/#7438).
        let guide_count = if command == "sketch.polygon.circumscribed" { 2 } else { 1 };
        assert_eq!(record.sketch.curves.len(), 4 + guide_count, "{command}");
        let created = out["created"]["curves"].as_array().unwrap();
        assert_eq!(created.len(), 4 + guide_count, "{command}: four sides plus construction guides");
        assert_eq!(
            created.iter().map(|id| id.as_str().unwrap()).collect::<Vec<_>>(),
            record.sketch.curves.iter().map(|curve| curve.id.as_str()).collect::<Vec<_>>(),
            "{command}: every saved side and guide is reported once"
        );
        assert!(created.iter().any(|id| id.as_str() == Some(support)), "{command}: construction circle is reported");
        let lines = record.sketch.curves.iter().filter(|c| matches!(c.kind, CurveKind::Line { .. })).count();
        assert_eq!(lines, 4, "{command}");
        assert!(record.sketch.curves.iter().filter(|c| matches!(c.kind, CurveKind::Line { .. })).all(|c| !c.construction));
        assert_eq!(record.sketch.curves.iter().filter(|c| c.construction && matches!(c.kind, CurveKind::Circle { .. })).count(), guide_count);
        let circle = record.sketch.curves.iter().find(|c| c.id == support).unwrap();
        assert!(circle.construction, "{command}: the radius guide is construction");
        let CurveKind::Circle { c, r } = circle.kind else { panic!("{command}: missing support circle") };
        assert!(record.sketch.points[c].pos.dist(centre) < 1e-7, "{command}: centre");
        assert!((r - radius).abs() < 1e-7, "{command}: radius");
        for at in vertices {
            assert!(record.sketch.points.iter().any(|p| p.pos.dist(Vec2::new(at[0], at[1])) < 1e-7), "{command}: {at:?}");
        }
        if command == "sketch.polygon.circumscribed" {
            let support_index = record.sketch.curve_index(support).unwrap();
            let (corner_index, corner_circle) = record.sketch.curves.iter().enumerate().find(|(_, c)| c.id != support && c.construction).unwrap();
            let CurveKind::Circle { c, r } = corner_circle.kind else { panic!("{command}: missing corner guide circle") };
            assert!(record.sketch.points[c].pos.dist(Vec2::new(60.0, 45.0)) < 1e-7, "{command}: corner guide centre");
            assert!((r - 10.0 * 2.0_f64.sqrt()).abs() < 1e-7, "{command}: corner guide radius");
            assert!(
                record.sketch.constraints.iter().any(|constraint| matches!(
                    constraint.kind,
                    ConstraintKind::Concentric { a, b } if (a == support_index && b == corner_index) || (a == corner_index && b == support_index)
                )),
                "{command}: the two guides must remain concentric"
            );
            let corners: std::collections::BTreeSet<usize> = record
                .sketch
                .curves
                .iter()
                .filter_map(|curve| if let CurveKind::Line { a, b } = curve.kind { Some([a, b]) } else { None })
                .flatten()
                .collect();
            assert_eq!(corners.len(), 4, "{command}: exactly four shared polygon corners");
            for point in corners {
                assert!(
                    record.sketch.constraints.iter().any(|constraint| matches!(
                        constraint.kind, ConstraintKind::PointOnCurve { p, c } if p == point && c == corner_index
                    )),
                    "{command}: corner {point} must stay on its guide"
                );
            }
            assert!(
                record.sketch.constraints.iter().any(|constraint| {
                    let ConstraintKind::Tangent { a, b } = constraint.kind else { return false };
                    let side = if a == support_index {
                        b
                    } else if b == support_index {
                        a
                    } else {
                        return false;
                    };
                    record.sketch.curves.get(side).is_some_and(|curve| !curve.construction && matches!(curve.kind, CurveKind::Line { .. }))
                }),
                "{command}: one side must remain tangent to the incircle"
            );
        }
        let accepted = record.sketch.clone();
        assert_eq!((&s.doc.features, &s.doc.params), (&before.features, &before.params));
        assert_eq!(s.evaluations, evaluated);
        assert!(Arc::ptr_eq(&model, &s.model.state()));
        assert_eq!((s.undo.len(), s.undo.last().unwrap().part), (undo + 1, Some(d)));
        run(&mut s, "edit.undo", json!({}));
        assert!(s.doc.drawing(d).unwrap().sheet_sketch(sk).unwrap().sketch.curves.is_empty());
        run(&mut s, "edit.redo", json!({}));
        assert_eq!(s.doc.drawing(d).unwrap().sheet_sketch(sk).unwrap().sketch, accepted);
        run(&mut s, "sketch.finish", json!({}));
        let saved = s.doc.to_json();
        let mut reopened = Session::new(solidcraft_doc::Document::from_json(&saved).unwrap());
        run(&mut reopened, "drawing.open", json!({"drawing":made["drawing"]}));
        run(&mut reopened, "drawing.sketch.edit", json!({"sketch":made["sketch"]}));
        assert_eq!(run(&mut reopened, "drawing.sketch.inspect", json!({}))["curves"].as_array().unwrap().len(), 4 + guide_count);
        assert_eq!(reopened.doc.drawing(d).unwrap().sheet_sketch(sk).unwrap().sketch, accepted);
        assert_eq!(reopened.doc.to_json(), saved);
    }
}

#[test]
fn sheet_next_slots_keep_tangent_caps_and_drawing_local_undo() {
    use solidcore_record::Vec2;
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
        assert_eq!(result["curves"].as_array().unwrap().len(), 4);
        assert_eq!(result["created"]["curves"].as_array().unwrap().len(), 5);
        assert_eq!(sketch.curves.len(), 5, "{command}: four sides and the construction centre line");
        assert_eq!(sketch.curves.iter().filter(|c| !c.construction && matches!(c.kind, CurveKind::Line { .. })).count(), 2);
        let guides: Vec<_> = sketch.curves.iter().enumerate().filter(|(_, c)| c.construction).collect();
        assert_eq!(guides.len(), 1);
        let (axis, guide) = guides[0];
        assert_eq!(result["centre_line"].as_str(), Some(guide.id.as_str()));
        let CurveKind::Line { a, b } = guide.kind else { panic!("expected slot centre line") };
        assert!(sketch.points[a].pos.dist(centres[0]) < 1e-7 && sketch.points[b].pos.dist(centres[1]) < 1e-7);
        let middle = sketch.resolve_point(result["middle"].as_str().unwrap()).unwrap();
        assert!(sketch.points[middle].pos.dist((centres[0] + centres[1]) * 0.5) < 1e-7);
        assert!(
            sketch.constraints.iter().any(|c| matches!(c.kind, solidcraft_sketch::ConstraintKind::Midpoint { p, l } if p == middle && l == axis))
        );
        let accepted = sketch.clone();
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
        assert_eq!(s.doc.drawing(d).unwrap().sheet_sketch(sk).unwrap().sketch, accepted);
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
    use solidcore_record::Vec2;
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
fn sheet_reviewer_coincident_lines_lie_along_one_line_on_their_sheet() {
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
    // Coincident on two lines lays them along one line (#18281), on a sheet as in a model sketch.
    assert!(matches!(matched.kind, solidcraft_sketch::ConstraintKind::Collinear { .. }), "{:?}", matched.kind);
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
    let before_centerline = s.doc.clone();
    let history = (s.undo.len(), s.redo.len(), s.evaluations);
    refused(&mut s, "sketch.centerline", json!({"curves":[a],"value":true}));
    assert!(Arc::ptr_eq(&s.doc, &before_centerline));
    assert_eq!((s.undo.len(), s.redo.len(), s.evaluations), history);
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
    for id in ["sketch.text", "sketch.edit_text", "sketch.offset", "sketch.pattern.circular", "sketch.pattern.rectangular", "sketch.drag_end"] {
        assert!(find_command(id).unwrap().allowed(&s).is_err(), "{id} is not admitted on a sheet");
    }
    // Entering a model sketch is the model's command, which a sheet sketch never holds: it ends
    // the sheet sketch instead (`a_sheet_sketch_holds_no_model_command`).
    for id in ["sketch.create", "sketch.edit"] {
        assert!(find_command(id).unwrap().allowed(&s).is_ok(), "{id} is the model's and runs");
    }
}

/// Each newly admitted point parser keeps paper-mm literals under inch source units,
/// while its model path still interprets bare strings in the model's design unit.
#[test]
fn sheet_batch_typed_creator_points_use_paper_units_without_changing_model_units() {
    use solidcore_record::Vec2;

    fn typed(value: &mut Value, source: bool) {
        match value {
            Value::Array(values) => {
                for value in values {
                    typed(value, source);
                }
            }
            Value::Number(number) => {
                let number = number.as_f64().unwrap();
                *value = json!(if source { format!("Inset + {} mm", number - 25.4) } else { number.to_string() });
            }
            _ => {}
        }
    }

    for (command, numeric, keys, points) in [
        ("sketch.line.midpoint", json!({"mid":[40,40],"end":[60,40]}), vec!["mid"], vec![[40.0, 40.0], [20.0, 40.0], [60.0, 40.0]]),
        (
            "sketch.conic",
            json!({"start":[40,40],"end":[90,40],"apex":[65,65],"rho":0.5,"connect":false}),
            vec!["start", "end", "apex"],
            vec![[40.0, 40.0], [90.0, 40.0], [65.0, 65.0]],
        ),
        (
            "sketch.spline.fit_point",
            json!({"points":[[40,40],[50,60],[70,55],[85,40]],"connect":false}),
            vec!["points"],
            vec![[40.0, 40.0], [50.0, 60.0], [70.0, 55.0], [85.0, 40.0]],
        ),
        (
            "sketch.spline.control_point",
            json!({"points":[[40,40],[50,60],[70,55],[85,40]],"connect":false}),
            vec!["points"],
            vec![[40.0, 40.0], [50.0, 60.0], [70.0, 55.0], [85.0, 40.0]],
        ),
        (
            "sketch.spline.control_point_5",
            json!({"points":[[40,40],[50,60],[55,65],[70,65],[80,55],[85,40]],"connect":false}),
            vec!["points"],
            vec![[40.0, 40.0], [50.0, 60.0], [55.0, 65.0], [70.0, 65.0], [80.0, 55.0], [85.0, 40.0]],
        ),
    ] {
        for mode in 0..3 {
            let (mut s, drawing, sketch, _) = sheet_editor();
            s.doc_mut().set_param("Inset", "1", Some("in"), None).unwrap();
            let source = s.doc.clone();
            let model = s.model.state();
            let history = s.undo.len();
            let evaluations = s.evaluations;
            let mut params = numeric.clone();
            if mode != 0 {
                for key in &keys {
                    typed(params.get_mut(*key).unwrap(), mode == 2);
                }
            }
            run(&mut s, command, params);
            let record = s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch.clone();
            for point in &points {
                assert!(record.points.iter().any(|p| p.pos.dist(Vec2::new(point[0], point[1])) < 1e-6), "{command} mode {mode}: {point:?}");
            }
            assert_eq!((&s.doc.features, &s.doc.params, &s.doc.units), (&source.features, &source.params, &source.units), "{command}");
            assert_eq!((s.undo.len(), s.undo.last().unwrap().part), (history + 1, Some(drawing)), "{command}");
            assert_eq!(s.evaluations, evaluations);
            assert!(Arc::ptr_eq(&s.model.state(), &model), "{command}");
            let reopened = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
            assert_eq!(reopened.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch, record, "{command}");
            if mode == 2 {
                assert!(record.points.iter().any(|p| p.exprs.is_some()), "{command}: authored source expressions survive");
                s.doc_mut().set_param("Inset", "2", Some("in"), None).unwrap();
                let before_solve = s.doc.clone();
                let solved = s.doc.solve_sheet_sketch(drawing, sketch).unwrap();
                assert!(solved.sketch.points.iter().any(|p| p.pos.dist(Vec2::new(65.4, 65.4)) < 1e-6), "{command}: fresh source position");
                assert!(Arc::ptr_eq(&s.doc, &before_solve), "{command}: read-only resolution");
                assert_eq!(s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch, record, "{command}: stored expression not baked");
            }
        }
        for as_text in [false, true] {
            let mut s = Session::default();
            s.doc_mut().units = "in".into();
            run(&mut s, "sketch.create", json!({"plane":"XY"}));
            let mut params = numeric.clone();
            if as_text {
                for key in &keys {
                    typed(params.get_mut(*key).unwrap(), false);
                }
            }
            run(&mut s, command, params);
            let sketch = s.doc.sketch(s.active_sketch.unwrap()).unwrap();
            let expected = if as_text { 1016.0 } else { 40.0 };
            assert!(
                sketch.points.iter().any(|p| p.pos.dist(Vec2::new(expected, expected)) < 1e-6),
                "{command}: original model units, text={as_text}"
            );
        }
    }
}

/// A successful compound paper edit ends only its own redo branch. Failed compounds
/// retain every owner, including a dimension failure after its creator has run.
#[test]
fn sheet_batch_compound_edits_preserve_foreign_redo_and_refuse_atomically() {
    fn same_redo(s: &Session, before: &[crate::Snapshot]) {
        assert_eq!(s.redo.len(), before.len());
        for (after, before) in s.redo.iter().zip(before) {
            assert_eq!((&after.label, after.part), (&before.label, before.part));
            assert!(Arc::ptr_eq(&after.doc, &before.doc));
        }
    }

    for (command, params) in [
        ("sketch.rectangle.rounded", json!({"p0":[100,100],"p1":[160,140],"radius":5})),
        (
            "sketch.draw.dimensioned",
            json!({"command":"sketch.line","params":{"points":[[100,100],[150,100]],"connect":false},"dimensions":{"length":40}}),
        ),
    ] {
        let (mut s, drawing, sketch, made) = sheet_editor();
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "drawing.close", json!({}));
        let b = run(&mut s, "drawing.create", json!({"name":"Other"}));
        let other = ids::decode(b["id"].as_str().unwrap()).unwrap();
        run(&mut s, "drawing.set", json!({"name":"Other future"}));
        run(&mut s, "edit.undo", json!({}));
        run(&mut s, "drawing.close", json!({}));
        run(&mut s, "parameters.add", json!({"name":"ModelFuture","expression":"7 mm"}));
        run(&mut s, "edit.undo", json!({}));
        run(&mut s, "drawing.open", the(drawing));
        run(&mut s, "drawing.sketch.edit", json!({"sketch":made["sketch"]}));
        run(&mut s, "sketch.point", json!({"point":[40,0],"id":"anchor"}));
        run(&mut s, "sketch.constraint.fix", json!({"entities":["anchor"]}));
        run(&mut s, "drawing.set", json!({"name":"Paper future"}));
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(s.redo.iter().map(|step| step.part).collect::<Vec<_>>(), vec![Some(other), None, Some(drawing)]);
        let redos = s.redo.clone();
        let before = s.doc.clone();
        let entry = s.active_sheet_sketch.clone();
        let model = s.model.state();
        let history = s.undo.len();
        let evaluations = s.evaluations;
        let failed = if command == "sketch.rectangle.rounded" {
            json!({"p0":[100,100],"p1":[160,140],"radius":25})
        } else {
            // Both endpoints are fixed. The nested line creator succeeds, but the
            // requested 5 mm driving dimension conflicts with their 40 mm distance.
            json!({"command":"sketch.line","params":{"points":["origin","anchor"],"connect":false,"infer":false},"dimensions":{"length":5}})
        };
        // Failure on a scratch must roll back its private owner provenance too:
        // adopting the unchanged failed attempt must preserve every old redo branch.
        let mut failed_attempt = s.scratch();
        refused(&mut failed_attempt, command, failed);
        s.accept_computed(failed_attempt).unwrap();
        assert!(Arc::ptr_eq(&s.doc, &before), "{command}");
        assert!(Arc::ptr_eq(&s.model.state(), &model), "{command}");
        assert_eq!(s.active_sheet_sketch, entry, "{command}");
        assert_eq!((s.undo.len(), s.evaluations), (history, evaluations), "{command}");
        same_redo(&s, &redos);

        run(&mut s, command, params);
        assert_eq!((s.undo.len(), s.undo.last().unwrap().part), (history + 1, Some(drawing)), "{command}: one own undo");
        same_redo(&s, &redos[..2]);
        assert_eq!(s.evaluations, evaluations);
        assert_eq!((&s.doc.features, &s.doc.params), (&before.features, &before.params), "{command}");
        assert!(Arc::ptr_eq(&s.model.state(), &model));
        let edited = s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().clone();
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch.curves.len(), 0);
        run(&mut s, "edit.redo", json!({}));
        assert_eq!(s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch, edited.sketch);
        same_redo(&s, &redos[..2]);
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "drawing.open", the(other));
        run(&mut s, "edit.redo", json!({}));
        assert_eq!(s.doc.drawing(other).unwrap().name, "Other future");
        assert_eq!(s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch, edited.sketch);
        run(&mut s, "drawing.close", json!({}));
        run(&mut s, "edit.redo", json!({}));
        assert!(s.doc.params.iter().any(|p| p.name == "ModelFuture"));
        assert_eq!(s.doc.drawing(other).unwrap().name, "Other future");
        assert_eq!(s.doc.drawing(drawing).unwrap().sheet_sketch(sketch).unwrap().sketch, edited.sketch);
    }
}

/// Provenance must outlive MAX_UNDO eviction, nested compound history clearing and
/// tab switches, while a checkpoint restores the matching branch provenance.
#[test]
fn sheet_batch_computed_history_replaces_each_touched_owner_only() {
    fn same_redo(s: &Session, redos: &[crate::Snapshot]) {
        assert_eq!(s.redo.len(), redos.len());
        for (after, before) in s.redo.iter().zip(redos) {
            assert_eq!((&after.label, after.part), (&before.label, before.part));
            assert!(Arc::ptr_eq(&after.doc, &before.doc));
        }
    }

    let mut s = design();
    let a = run(&mut s, "drawing.create", json!({"name":"A"}));
    let aid = ids::decode(a["id"].as_str().unwrap()).unwrap();
    run(&mut s, "drawing.set", json!({"name":"A old future"}));
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "drawing.close", json!({}));
    let b = run(&mut s, "drawing.create", json!({"name":"B"}));
    let bid = ids::decode(b["id"].as_str().unwrap()).unwrap();
    run(&mut s, "drawing.set", json!({"name":"B old future"}));
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "drawing.close", json!({}));
    let c = run(&mut s, "drawing.create", json!({"name":"C"}));
    let cid = ids::decode(c["id"].as_str().unwrap()).unwrap();
    let made = run(&mut s, "drawing.sketch.create", json!({"owner":c["sheets"][0]["id"]}));
    let sketch = ids::decode(made["sketch"].as_str().unwrap()).unwrap();
    run(&mut s, "sketch.point", json!({"point":[5,7]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "drawing.set", json!({"name":"C old future"}));
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "parameters.add", json!({"name":"ModelFuture","expression":"7 mm"}));
    run(&mut s, "edit.undo", json!({}));
    let model_redo = s.redo.iter().find(|step| step.part.is_none()).unwrap().clone();
    let redos = s.redo.clone();
    run(&mut s, "drawing.open", the(aid));
    let before_read = s.doc.clone();
    let mut read_only = s.scratch();
    run(&mut read_only, "drawing.open", the(bid));
    run(&mut read_only, "drawing.inspect", json!({}));
    s.accept_computed(read_only).unwrap();
    assert_eq!(s.open_drawing, Some(bid));
    assert!(Arc::ptr_eq(&s.doc, &before_read));
    same_redo(&s, &redos);

    // A complete rollback, even across file.new, restores the old provenance just
    // as it restores the old document and undo/redo. The rolled-back edits must
    // not end the parent's legitimate redo branches at later adoption.
    run(&mut s, "drawing.open", the(aid));
    let mut rolled_back = s.scratch();
    let checkpoint = run(&mut rolled_back, "edit.checkpoint", json!({}))["checkpoint"].clone();
    run(&mut rolled_back, "drawing.set", json!({"name":"A discarded"}));
    run(&mut rolled_back, "drawing.open", the(bid));
    run(&mut rolled_back, "drawing.set", json!({"name":"B discarded"}));
    run(&mut rolled_back, "file.new", json!({}));
    assert!(rolled_back.doc.drawings.is_empty());
    run(&mut rolled_back, "edit.restore_checkpoint", json!({"checkpoint":checkpoint}));
    assert!(Arc::ptr_eq(&rolled_back.doc, &s.doc));
    s.accept_computed(rolled_back).unwrap();
    same_redo(&s, &redos);

    let mut computed = s.scratch();
    run(&mut computed, "drawing.set", json!({"name":"A new"}));
    run(&mut computed, "drawing.open", the(bid));
    for i in 0..=crate::MAX_UNDO {
        run(&mut computed, "drawing.set", json!({"name":format!("B new{i}")}));
    }
    assert_eq!(computed.undo.len(), crate::MAX_UNDO);
    assert!(computed.undo.iter().all(|step| step.part == Some(bid)), "A's edit really was evicted");

    // The only edit of C happens in a nested scratch and a rounded compound.
    // Erasing its intermediate history and finishing on untouched B must not
    // erase C's owner evidence or mistake B for the compound's actual owner.
    let mut nested = computed.scratch();
    run(&mut nested, "drawing.open", the(cid));
    run(&mut nested, "drawing.sketch.edit", json!({"sketch":made["sketch"]}));
    run(&mut nested, "sketch.rectangle.rounded", json!({"p0":[20,20],"p1":[60,40],"radius":3}));
    nested.undo.clear();
    run(&mut nested, "sketch.finish", json!({}));
    run(&mut nested, "drawing.open", the(bid));
    assert!(nested.undo.is_empty() && nested.redo.is_empty());
    computed.accept_computed(nested).unwrap();
    assert!(computed.undo.iter().all(|step| step.part == Some(bid)), "neither A nor C survives in retained history");
    s.accept_computed(computed).unwrap();
    assert_eq!(s.open_drawing, Some(bid));
    assert_eq!(s.doc.drawing(aid).unwrap().name, "A new");
    assert_eq!(s.doc.drawing(bid).unwrap().name, format!("B new{}", crate::MAX_UNDO));
    let retained = s.doc.drawing(cid).unwrap().sheet_sketch(sketch).unwrap().sketch.clone();
    assert_eq!(retained.curves.len(), 9, "eight rounded-rectangle sides and the construction diagonal");
    assert_eq!(retained.curves.iter().filter(|curve| !curve.construction).count(), 8);
    assert_eq!(retained.curves.iter().filter(|curve| curve.construction).count(), 1);
    assert_eq!(s.redo.len(), 1);
    assert!(s.redo[0].part.is_none());
    assert!(Arc::ptr_eq(&s.redo[0].doc, &model_redo.doc));
    run(&mut s, "drawing.open", the(aid));
    refused(&mut s, "edit.redo", json!({}));
    assert_eq!(s.doc.drawing(aid).unwrap().name, "A new", "old A future cannot overwrite the new edit");
    run(&mut s, "drawing.open", the(cid));
    refused(&mut s, "edit.redo", json!({}));
    assert_eq!(s.doc.drawing(cid).unwrap().sheet_sketch(sketch).unwrap().sketch, retained);
    run(&mut s, "drawing.open", the(bid));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.drawing(bid).unwrap().name, format!("B new{}", crate::MAX_UNDO - 1));
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(s.doc.drawing(bid).unwrap().name, format!("B new{}", crate::MAX_UNDO));
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "edit.redo", json!({}));
    assert!(s.doc.params.iter().any(|p| p.name == "ModelFuture"));
    assert_eq!(s.doc.drawing(aid).unwrap().name, "A new");
    assert_eq!(s.doc.drawing(bid).unwrap().name, format!("B new{}", crate::MAX_UNDO));
    assert_eq!(s.doc.drawing(cid).unwrap().sheet_sketch(sketch).unwrap().sketch, retained);
}

#[test]
fn sheet_batch_computed_visibility_invalidates_model_redo_after_history_eviction() {
    let mut s = design();
    let a = run(&mut s, "drawing.create", json!({"name":"A"}));
    let aid = ids::decode(a["id"].as_str().unwrap()).unwrap();
    run(&mut s, "drawing.close", json!({}));
    let b = run(&mut s, "drawing.create", json!({"name":"B"}));
    let bid = ids::decode(b["id"].as_str().unwrap()).unwrap();
    run(&mut s, "drawing.set", json!({"name":"B future"}));
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "parameters.add", json!({"name":"ModelFuture","expression":"7 mm"}));
    run(&mut s, "edit.undo", json!({}));
    let redos = s.redo.clone();
    assert!(redos.iter().any(|step| step.part.is_none()));
    let b_redo = redos.iter().find(|step| step.part == Some(bid)).unwrap().clone();

    // Visibility has its own history even though it does not change the design
    // Arc. Checkpoint restoration must restore its branch provenance too.
    let mut rolled_back = s.scratch();
    let checkpoint = run(&mut rolled_back, "edit.checkpoint", json!({}))["checkpoint"].clone();
    let original_visibility = rolled_back.visibility.clone();
    let original_document = rolled_back.doc.clone();
    run(&mut rolled_back, "browser.visibility", json!({"items":["Plate"],"visible":false}));
    assert!(Arc::ptr_eq(&rolled_back.doc, &original_document));
    assert_ne!(rolled_back.visibility, original_visibility);
    let changed_visibility = rolled_back.visibility.clone();
    refused(&mut rolled_back, "browser.visibility", json!({"items":["Plate","Missing"],"visible":true}));
    assert_eq!(rolled_back.visibility, changed_visibility, "a refused visibility edit is atomic");
    run(&mut rolled_back, "edit.restore_checkpoint", json!({"checkpoint":checkpoint}));
    assert_eq!(rolled_back.visibility, original_visibility);
    s.accept_computed(rolled_back).unwrap();
    assert_eq!(s.redo.len(), redos.len());
    for (after, before) in s.redo.iter().zip(&redos) {
        assert_eq!((&after.label, after.part), (&before.label, before.part));
        assert!(Arc::ptr_eq(&after.doc, &before.doc));
    }

    let mut computed = s.scratch();
    let original_document = computed.doc.clone();
    let original_visibility = computed.visibility.clone();
    run(&mut computed, "browser.visibility", json!({"items":["Plate"],"visible":false}));
    assert!(Arc::ptr_eq(&computed.doc, &original_document));
    assert_ne!(computed.visibility, original_visibility);
    assert!(computed.undo.iter().any(|step| step.part.is_none()));
    run(&mut computed, "drawing.open", the(aid));
    for i in 0..=crate::MAX_UNDO {
        run(&mut computed, "drawing.set", json!({"name":format!("A changed{i}")}));
    }
    assert_eq!(computed.undo.len(), crate::MAX_UNDO);
    assert!(computed.undo.iter().all(|step| step.part == Some(aid)), "the model visibility snapshot really was evicted");
    s.accept_computed(computed).unwrap();
    assert_eq!(s.redo.len(), 1, "the stale model future was invalidated");
    assert_eq!(s.redo[0].part, Some(bid));
    assert!(Arc::ptr_eq(&s.redo[0].doc, &b_redo.doc), "the unrelated drawing future was retained");
    run(&mut s, "drawing.close", json!({}));
    refused(&mut s, "edit.redo", json!({}));
    assert!(!s.doc.params.iter().any(|p| p.name == "ModelFuture"));
    run(&mut s, "drawing.open", the(bid));
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(s.doc.drawing(bid).unwrap().name, "B future");
    assert_eq!(s.doc.drawing(aid).unwrap().name, format!("A changed{}", crate::MAX_UNDO));
}

#[test]
fn sheet_editor_model_cancel_refuses_foreign_drawing_changes_without_losing_them() {
    fn same_steps(after: &[crate::Snapshot], before: &[crate::Snapshot]) {
        assert_eq!(after.len(), before.len());
        for (after, before) in after.iter().zip(before) {
            assert_eq!((&after.label, after.part, after.active_sketch), (&before.label, before.part, before.active_sketch));
            assert!(Arc::ptr_eq(&after.doc, &before.doc));
            assert_eq!(after.active_sheet_sketch, before.active_sheet_sketch);
            assert_eq!(after.visibility, before.visibility);
            assert_eq!(after.pending_moves, before.pending_moves);
        }
    }

    // Creating a drawing changes the model-owned membership list (part None),
    // while editing an existing drawing changes a drawing-owned part. Both must
    // be protected by the model's whole-document cancellation boundary.
    for scenario in ["new drawing", "new drawing with sheet edits", "existing drawing"] {
        let mut s = design();
        let existing = if scenario == "existing drawing" {
            let drawing = run(&mut s, "drawing.create", json!({"name":"Original paper"}));
            let made = run(&mut s, "drawing.sketch.create", json!({"owner":drawing["sheets"][0]["id"]}));
            run(&mut s, "sketch.line", json!({"points":[[10,10],[30,10]],"connect":false}));
            run(&mut s, "sketch.finish", json!({}));
            run(&mut s, "drawing.close", json!({}));
            Some((drawing, made))
        } else {
            None
        };
        let entered = run(&mut s, "sketch.create", json!({"plane":"XY","name":"Unfinished model sketch"}));
        let model_sketch = entered["sketch"].as_u64().unwrap();
        run(&mut s, "sketch.line", json!({"points":[[0,0],[40,0]],"connect":false}));
        if let Some((drawing, made)) = existing {
            run(&mut s, "drawing.open", json!({"drawing":drawing["id"]}));
            run(&mut s, "drawing.set", json!({"name":"Revised paper"}));
            run(&mut s, "drawing.sketch.edit", json!({"sketch":made["sketch"]}));
            run(&mut s, "sketch.point", json!({"point":[55,25],"id":"new_sheet_point","connect":false}));
            run(&mut s, "sketch.finish", json!({}));
        } else {
            let drawing = run(&mut s, "drawing.create", json!({"name":"New paper"}));
            if scenario == "new drawing with sheet edits" {
                run(&mut s, "drawing.sketch.create", json!({"owner":drawing["sheets"][0]["id"]}));
                run(&mut s, "sketch.line", json!({"points":[[15,10],[55,10]],"connect":false}));
                run(&mut s, "sketch.finish", json!({}));
            }
        }
        run(&mut s, "drawing.close", json!({}));
        if scenario != "new drawing" {
            assert!(s.active_sketch.is_none(), "entering the sheet editor finished the model sketch");
            assert!(s.doc.feature(model_sketch).is_some());
            let before = s.doc.clone();
            refused(&mut s, "sketch.cancel", json!({}));
            assert!(Arc::ptr_eq(&s.doc, &before), "Cancel cannot erase the completed sketch or drawing edits");
            continue;
        }
        assert_eq!(s.active_sketch, Some(model_sketch));
        let before = s.doc.clone();
        let model = s.model.state();
        let undo = s.undo.clone();
        let redo = s.redo.clone();
        let entry = s.sketch_entry.clone().unwrap();
        let evaluations = s.evaluations;
        let was_stale = s.model.is_stale();
        let why = refused(&mut s, "sketch.cancel", json!({}));
        assert!(why.contains("undo history"), "{scenario}: {why}");
        assert!(Arc::ptr_eq(&s.doc, &before), "{scenario}: Cancel lost drawing work");
        assert!(Arc::ptr_eq(&s.model.state(), &model), "{scenario}: Cancel recomputed the model");
        assert_eq!(s.active_sketch, Some(model_sketch));
        assert!(s.active_sheet_sketch.is_none() && s.open_drawing.is_none());
        assert_eq!((s.evaluations, s.model.is_stale()), (evaluations, was_stale));
        same_steps(&s.undo, &undo);
        same_steps(&s.redo, &redo);
        let retained_entry = s.sketch_entry.as_ref().unwrap();
        assert_eq!((retained_entry.sketch, retained_entry.undo_len), (entry.sketch, entry.undo_len));
        assert!(Arc::ptr_eq(&retained_entry.doc, &entry.doc));
        assert_eq!(s.doc.drawings, before.drawings);
        run(&mut s, "sketch.finish", json!({}));
        assert!(s.active_sketch.is_none() && s.sketch_entry.is_none());
        assert!(Arc::ptr_eq(&s.doc, &before), "{scenario}: Finish changed completed drawing work");
        assert_eq!(s.doc.drawings, before.drawings);
        same_steps(&s.undo, &undo);
        same_steps(&s.redo, &redo);
        assert_eq!((s.evaluations, s.model.is_stale()), (evaluations, was_stale));
    }
}

#[test]
fn sheet_editor_post_publication_refusal_preserves_stale_model_and_history() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane":"XY"}));
    let model_sketch = s.active_sketch.unwrap();
    let line = run(&mut s, "sketch.line", json!({"points":[[0,0],[40,0]]}))["curves"][0].as_str().unwrap().to_string();
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.box", json!({"length":5,"width":5,"height":5}));
    run(&mut s, "sketch.edit", json!({"sketch":model_sketch}));
    run(&mut s, "sketch.drag", json!({"entity":format!("{line}.end"),"from":[40,0],"to":[45,3],"live":true}));
    assert!(s.model.is_stale(), "the source has an unfinished live gesture");
    let drawing = run(&mut s, "drawing.create", json!({}));
    let made = run(&mut s, "drawing.sketch.create", json!({"owner":drawing["sheets"][0]["id"]}));
    let paper_line_result = run(&mut s, "sketch.line", json!({"points":[[15,10],[55,10]],"connect":false,"infer":false}));
    let paper_line = paper_line_result["curves"][0].clone();
    // Fix the line rather than adding Horizontal first: an exact
    // duplicate would be deduplicated before publishing any changed sketch.
    run(&mut s, "sketch.constraint.fix", json!({"entities":[paper_line],"fixed":true}));
    run(&mut s, "sketch.point", json!({"point":[80,30],"id":"redo_sheet_point","connect":false}));
    run(&mut s, "edit.undo", json!({}));
    assert!(!s.redo.is_empty(), "the rejected edit must also preserve a valid drawing redo");
    let before = s.doc.clone();
    let model = s.model.state();
    let undo = s.undo.clone();
    let redo = s.redo.clone();
    let entry = s.active_sheet_sketch.clone();
    let evaluations = s.evaluations;
    let visibility = s.visibility.clone();
    let pending_moves = s.pending_moves.clone();
    assert!(s.model.is_stale());
    // A nested handler has no outer Session rollback: prove this refusal occurs
    // after a distinct constraint was published, rather than on a no-change
    // duplicate. The real execute below must restore that transient publication.
    let sketch_id = ids::decode(made["sketch"].as_str().unwrap()).unwrap();
    let constraint_count = s.drawing("test").unwrap().sheet_sketch(sketch_id).unwrap().sketch.constraints.len();
    let refusal = json!({"line":paper_line,"mode":"horizontal","if_redundant":"refuse"});
    let mut probe = s.scratch();
    let handler_error = find_command("sketch.constraint.horizontal_vertical").unwrap().run_nested(&mut probe, &refusal).unwrap_err();
    assert!(handler_error.to_string().contains("over-constrain"));
    assert!(!Arc::ptr_eq(&probe.doc, &before));
    assert_eq!(probe.drawing("test").unwrap().sheet_sketch(sketch_id).unwrap().sketch.constraints.len(), constraint_count + 1);
    let why = refused(&mut s, "sketch.constraint.horizontal_vertical", refusal);
    assert!(why.contains("over-constrain"), "the new redundant constraint must reach the post-publication refusal: {why}");
    assert!(Arc::ptr_eq(&s.doc, &before));
    assert!(Arc::ptr_eq(&s.model.state(), &model), "a drawing-only refusal must not recompute its stale source");
    assert_eq!(s.active_sheet_sketch, entry);
    assert!(s.active_sketch.is_none(), "the sheet editor finished the earlier model edit");
    assert_eq!(s.open_drawing, ids::decode(made["drawing"].as_str().unwrap()));
    assert_eq!(s.evaluations, evaluations);
    assert!(s.model.is_stale());
    assert_eq!(s.visibility, visibility);
    assert_eq!(s.pending_moves, pending_moves);
    for (after, original) in [(&s.undo, &undo), (&s.redo, &redo)] {
        assert_eq!(after.len(), original.len());
        for (after, original) in after.iter().zip(original) {
            assert_eq!((&after.label, after.part, after.active_sketch), (&original.label, original.part, original.active_sketch));
            assert!(Arc::ptr_eq(&after.doc, &original.doc));
            assert_eq!(after.active_sheet_sketch, original.active_sheet_sketch);
            assert_eq!(after.visibility, original.visibility);
            assert_eq!(after.pending_moves, original.pending_moves);
        }
    }
    assert_eq!((&s.doc.features, &s.doc.params), (&before.features, &before.params));
    run(&mut s, "edit.redo", json!({}));
    let drawing = s.drawing("test").unwrap();
    let sketch = drawing.sheet_sketch(ids::decode(made["sketch"].as_str().unwrap()).unwrap()).unwrap();
    assert!(sketch.sketch.resolve_point("redo_sheet_point").is_some());
    assert!(Arc::ptr_eq(&s.model.state(), &model));
    assert_eq!(s.evaluations, evaluations);
    assert!(s.model.is_stale());
}

#[test]
fn sheet_editor_model_cancel_keeps_a_fully_undone_drawings_redo() {
    let mut s = design();
    let drawing = run(&mut s, "drawing.create", json!({"name":"Original paper"}));
    let drawing_id = ids::decode(drawing["id"].as_str().unwrap()).unwrap();
    run(&mut s, "drawing.close", json!({}));
    let entered = run(&mut s, "sketch.create", json!({"plane":"XY","name":"Cancelled model sketch"}));
    let model_sketch = entered["sketch"].as_u64().unwrap();
    run(&mut s, "sketch.line", json!({"points":[[0,0],[40,0]],"connect":false}));
    run(&mut s, "drawing.open", json!({"drawing":drawing["id"]}));
    run(&mut s, "drawing.set", json!({"name":"Drawing future"}));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.drawing(drawing_id).unwrap().name, "Original paper");
    let future = s.redo.iter().find(|step| step.part == Some(drawing_id)).unwrap().clone();
    run(&mut s, "drawing.close", json!({}));
    let cancelled = run(&mut s, "sketch.cancel", json!({}));
    assert_eq!(cancelled["removed"], true);
    assert!(s.doc.feature(model_sketch).is_none());
    assert!(s.active_sketch.is_none());
    let retained = s.redo.iter().find(|step| step.part == Some(drawing_id)).unwrap();
    assert_eq!((&retained.label, retained.part), (&future.label, future.part));
    assert!(Arc::ptr_eq(&retained.doc, &future.doc), "model Cancel must retain the drawing's own future");
    let model = s.doc.features.clone();
    run(&mut s, "drawing.open", json!({"drawing":drawing["id"]}));
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(s.doc.drawing(drawing_id).unwrap().name, "Drawing future");
    assert_eq!(s.doc.features, model, "drawing redo must not resurrect the cancelled model sketch");
    assert!(s.doc.feature(model_sketch).is_none());
    assert!(s.active_sketch.is_none());
}

#[test]
fn sheet_editor_restore_editor_preserves_ids_consumed_after_its_original() {
    let mut s = design();
    s.seed_ids(3938);
    let original = s.scratch();
    let retired = run(&mut s, "drawing.create", json!({"name":"Retired paper"}))["id"].clone();
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "drawing.delete", json!({"drawing":retired}));
    assert!(s.doc.drawings.is_empty());
    let source = s.scratch();
    let history = (s.undo.len(), s.redo.len());
    let evaluations = s.evaluations;
    s.restore_editor(&source, original).unwrap();
    assert_eq!((s.undo.len(), s.redo.len()), history);
    assert_eq!(s.evaluations, evaluations);
    assert!(s.doc.drawings.is_empty());
    let next = run(&mut s, "drawing.create", json!({"name":"Later paper"}));
    assert_ne!(next["id"], retired, "restoring an old editor model must not rewind the live id source");
}

/// Source A puts the inner rectangle wholly outside its former outer boundary;
/// B returns it to [25,35], while C moves it to [30,40]. The actual source fields
/// use millimetres even though sheet_editor's display/source units are inches.
fn w38d_source_history_fixture() -> (Session, u64, u64, solidcore_record::Region2) {
    let (mut s, drawing, _, made, boundary) = sheet_fill_fixture(true);
    s.doc_mut().set_param("FillHistoryShift", "40 mm", Some("mm"), None).unwrap();
    let mut owner = (**s.doc.drawing(drawing).unwrap()).clone();
    let record = Arc::make_mut(&mut owner.sketches[0]);
    let mut driven = 0;
    for point in record.sketch.points.iter_mut().skip(1) {
        if [25.0, 35.0].contains(&point.pos.x) && [15.0, 25.0].contains(&point.pos.y) {
            point.exprs = Some(Box::new(solidcraft_sketch::PointExprs { x: Some(format!("FillHistoryShift + {} mm", point.pos.x)), y: None }));
            driven += 1;
        }
    }
    assert_eq!(driven, 4);
    s.doc_mut().put_drawing(Arc::new(owner));
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "parameters.change", json!({"name":"FillHistoryShift","expression":"0 mm"}));
    run(&mut s, "drawing.open", the(drawing));
    let created = run(&mut s, "drawing.sketch.fill", json!({"sketch":made["sketch"],"boundary":boundary,"paint":{"kind":"solid"}}));
    let fill = ids::decode(created["fills"][0]["fill"].as_str().unwrap()).unwrap();
    let region = s.doc.drawing(drawing).unwrap().sheet_fill(fill).unwrap().last_region.clone();
    assert_eq!(region.holes.len(), 1);
    assert_eq!(region.holes[0].segs.iter().map(|seg| seg.start().x).fold(f64::INFINITY, f64::min), 25.0);
    run(&mut s, "drawing.close", json!({}));
    (s, drawing, fill, region)
}

fn w38d_history_resolved(s: &Session, drawing: u64, fill: u64) -> solidcraft_doc::drawing::ResolvedSketchFill {
    s.doc.drawing(drawing).unwrap().resolve_sheet_fill(&s.doc, fill).unwrap()
}

#[test]
fn sheet_fill_source_history_valid_valid_lost_undo_redo_keeps_the_last_valid_region() {
    use solidcraft_doc::drawing::FillRegionStatus;
    let (mut s, drawing, fill, at_b) = w38d_source_history_fixture();
    let drawing_steps = s.undo.iter().filter(|step| step.part == Some(drawing)).count();
    run(&mut s, "parameters.change", json!({"name":"FillHistoryShift","expression":"5 mm"}));
    let at_c = s.doc.drawing(drawing).unwrap().sheet_fill(fill).unwrap().last_region.clone();
    assert_ne!(at_c, at_b);
    assert_eq!(at_c.holes[0].segs.iter().map(|seg| seg.start().x).fold(f64::INFINITY, f64::min), 30.0);
    run(&mut s, "edit.undo", json!({})); // C -> B, valid -> valid.
    assert_eq!(w38d_history_resolved(&s, drawing, fill).status, FillRegionStatus::Current);
    assert_eq!(s.doc.drawing(drawing).unwrap().sheet_fill(fill).unwrap().last_region, at_b);
    run(&mut s, "edit.undo", json!({})); // B -> A, valid -> lost.
    let lost = w38d_history_resolved(&s, drawing, fill);
    assert!(matches!(lost.status, FillRegionStatus::LostBoundary(_)));
    assert_eq!(lost.region, at_b, "the retained hole must be B's, never the future C snapshot");
    run(&mut s, "edit.redo", json!({})); // A -> B.
    assert_eq!(w38d_history_resolved(&s, drawing, fill).region, at_b);
    run(&mut s, "edit.redo", json!({})); // B -> C.
    assert_eq!(w38d_history_resolved(&s, drawing, fill).region, at_c);
    assert_eq!(s.doc.drawing(drawing).unwrap().sheet_fill(fill).unwrap().last_region, at_c);
    assert_eq!(s.undo.iter().filter(|step| step.part == Some(drawing)).count(), drawing_steps, "source history adds no drawing steps");
}

#[test]
fn sheet_fill_source_checkpoint_restores_its_complete_lost_snapshot_and_history() {
    use solidcraft_doc::drawing::FillRegionStatus;
    let (mut s, drawing, fill, at_b) = w38d_source_history_fixture();
    run(&mut s, "parameters.change", json!({"name":"FillHistoryShift","expression":"40 mm"}));
    assert!(matches!(w38d_history_resolved(&s, drawing, fill).status, FillRegionStatus::LostBoundary(_)));
    let checkpoint = run(&mut s, "edit.checkpoint", json!({}))["checkpoint"].clone();
    let kept = s.checkpoints.len();
    let saved = s.doc.clone();
    let history = (s.undo.len(), s.redo.len());
    run(&mut s, "parameters.change", json!({"name":"FillHistoryShift","expression":"5 mm"}));
    let at_c = s.doc.drawing(drawing).unwrap().sheet_fill(fill).unwrap().last_region.clone();
    assert_ne!(at_c, at_b);
    run(&mut s, "edit.restore_checkpoint", json!({"checkpoint":checkpoint}));
    assert_eq!(s.checkpoints.len(), kept - 1);
    let lost = w38d_history_resolved(&s, drawing, fill);
    assert!(matches!(lost.status, FillRegionStatus::LostBoundary(_)));
    assert_eq!(lost.region, at_b, "whole checkpoint restoration keeps its own retained region, not later C");
    assert_eq!(s.doc.drawing(drawing).unwrap().sheet_fill(fill).unwrap().last_region, at_b);
    assert_eq!(*s.doc, *saved, "a checkpoint restores its complete saved Document");
    assert_eq!((s.undo.len(), s.redo.len()), history);
}

#[test]
fn sheet_fill_source_updates_preserve_independent_drawing_redo_and_its_latest_retention() {
    use solidcraft_doc::drawing::FillRegionStatus;
    let (mut s, drawing, fill, at_b) = w38d_source_history_fixture();
    run(&mut s, "drawing.open", the(drawing));
    run(&mut s, "drawing.pattern.import", json!({"kind":"builtin","pattern":"lines_45"}));
    run(&mut s, "edit.undo", json!({}));
    assert!(s.doc.drawing(drawing).unwrap().patterns.is_empty());
    let redo = s.redo.iter().find(|step| step.part == Some(drawing)).unwrap().clone();
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "parameters.change", json!({"name":"FillHistoryShift","expression":"5 mm"}));
    let at_c = s.doc.drawing(drawing).unwrap().sheet_fill(fill).unwrap().last_region.clone();
    assert_ne!(at_c, at_b);
    run(&mut s, "parameters.change", json!({"name":"FillHistoryShift","expression":"40 mm"}));
    let kept = s.redo.iter().find(|step| step.part == Some(drawing)).unwrap();
    assert_eq!(kept.label, redo.label);
    assert!(Arc::ptr_eq(&kept.doc, &redo.doc), "source edits preserve the independent drawing redo entry");
    assert_eq!(w38d_history_resolved(&s, drawing, fill).region, at_c);
    run(&mut s, "drawing.open", the(drawing));
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(s.doc.drawing(drawing).unwrap().patterns.len(), 1, "the unrelated pattern redo is still applied");
    let lost = w38d_history_resolved(&s, drawing, fill);
    assert!(matches!(lost.status, FillRegionStatus::LostBoundary(_)));
    assert_eq!(lost.region, at_c, "pattern-only redo must not rewind retained geometry to its older B snapshot");
}

/// A sheet sketch shares the drawing's complete object namespace, including image files
/// and placements retained only by its own undo/redo history.
#[test]
fn sheet_sketch_creation_reserves_current_and_retired_image_ids() {
    let folder = Folder::new("sheet-sketch-image-namespace");
    let file = folder.path("mark.png");
    std::fs::write(&file, crate::drawing_pictures::tests::png(2, 2)).unwrap();
    for retired in [false, true] {
        for candidate in [0, 1] {
            let mut s = design();
            let made = run(&mut s, "drawing.create", json!({}));
            let owner = ids::decode(made["id"].as_str().unwrap()).unwrap();
            let sheet = made["sheets"][0]["id"].clone();
            let before = s.drawing("test").unwrap().clone();
            let seed = 7_082_038;
            let mut source = IdSource::seeded(seed);
            let placement = before.allocate_id(&mut source).unwrap();
            let after_placement_seed = source.seed();
            let image_file = before.allocate_id(&mut source).unwrap();
            s.seed_ids(seed);
            let added = run(&mut s, "drawing.image.add", json!({"sheet": sheet, "file": file, "rect_mm": [10, 20, 12, 22]}));
            assert_eq!(added["image"], ids::encode(placement));
            let with_image = s.drawing("test").unwrap().clone();
            assert!(with_image.images.by_id(image_file).is_some());
            assert_eq!(with_image.sheet_images.by_id(placement).unwrap().image, image_file);
            if retired {
                run(&mut s, "edit.undo", json!({}));
                assert!(s.drawing("test").unwrap().images.is_empty() && s.drawing("test").unwrap().sheet_images.is_empty());
                assert!(s.redo.iter().any(|step| step.doc.drawing(owner).is_some_and(|d| d.images.by_id(image_file).is_some())));
            }
            // Force the next proposed id to be each actual imported record in turn.
            s.seed_ids(if candidate == 0 { seed } else { after_placement_seed });
            let (model, evaluations, features) = (s.model.state(), s.evaluations, s.doc.features.clone());
            let created = run(&mut s, "drawing.sketch.create", json!({"owner": sheet}));
            let sketch = ids::decode(created["sketch"].as_str().unwrap()).unwrap();
            assert!(
                ![placement, image_file].contains(&sketch),
                "current and history-only image ids stay reserved: retired={retired}, candidate={candidate}"
            );
            let drawing = s.drawing("test").unwrap();
            drawing.validate().unwrap();
            assert!(drawing.sheet_sketch(sketch).is_some());
            if !retired {
                assert_eq!(drawing.images, with_image.images);
                assert_eq!(drawing.sheet_images, with_image.sheet_images);
            }
            assert!(Arc::ptr_eq(&model, &s.model.state()));
            assert_eq!((s.evaluations, s.doc.features.clone()), (evaluations, features));
        }
    }
}

/// A model command started while a sheet sketch is being edited ends that sheet sketch as Finish
/// would (starting another edit finishes the open sketch), then runs as its own model step; the
/// model's undo leaves the sheet sketch finished. The model's command is never refused.
#[test]
fn a_sheet_sketch_holds_no_model_command() {
    let mut s = design();
    run(&mut s, "drawing.create", json!({}));
    let owner = ids::encode(open_one(&s).sheets[0].id);
    run(&mut s, "drawing.sketch.create", json!({"owner": owner}));
    let editing = s.active_sheet_sketch.clone();
    assert!(editing.is_some() && s.sheet_sketching());
    let features = s.doc.features.len();
    run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [60, 0, 0], "operation": "new"}));
    assert_eq!(s.doc.features.len(), features + 1, "the model changes under the sheet sketch");
    assert!(s.active_sheet_sketch.is_none(), "starting a model command ends the sheet sketch, as Finish would");
    assert_eq!(s.undo.last().unwrap().part, None, "the box is the model's step");
    s.model_focus = true;
    assert!(!s.sheet_sketching(), "with the model shown, the sketch tools are the model's");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.features.len(), features, "the model's undo");
    assert!(s.active_sheet_sketch.is_none(), "and the sheet sketch stays finished");
    s.model_focus = false;
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    assert!(s.active_sketch.is_some(), "the model's sketch is entered");
    assert!(s.active_sheet_sketch.is_none(), "and the sheet sketch is finished, its edits kept in the drawing");
}

/// An open drawing changes whether no model command can run: every `Part::Model` command is
/// allowed with the drawing open exactly when it is with the drawing closed, and Select All
/// (Ctrl+A) runs in Design (`model_focus`) with it open.
#[test]
fn an_open_drawing_allows_every_model_command_it_would_without_one() {
    let mut s = design();
    run(&mut s, "drawing.create", json!({}));
    let open = s.open_drawing;
    let with_drawing: Vec<_> = command_specs().iter().filter(|c| c.part == Part::Model).map(|c| (c.id, c.allowed(&s).is_ok())).collect();
    run(&mut s, "drawing.close", json!({}));
    let without: Vec<_> = command_specs().iter().filter(|c| c.part == Part::Model).map(|c| (c.id, c.allowed(&s).is_ok())).collect();
    assert_eq!(with_drawing, without);
    run(&mut s, "drawing.open", json!({"drawing": ids::encode(open.unwrap())}));
    s.model_focus = true;
    run(&mut s, "select.all", json!({}));
    assert!(!s.selection.is_empty(), "Select All in Design picks the model's bodies with a drawing open");
}
