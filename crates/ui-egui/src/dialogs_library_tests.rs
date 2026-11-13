use super::*;
use crate::{dialogs::Kind, scenario::Harness};
use solidcraft_engine::{Sel, Session};

#[test]
fn copied_catalog_id_labels_survive_clicked_editor_reopen_and_retargeting() {
    for name in ["starter.metal.gold-warm", "starter.glass.clear-soda-lime"] {
        let mut h = Harness::new();
        h.call("ui.resize", json!({"width":1600,"height":1400}));
        let path = h.dir.join("owned-label.json").to_string_lossy().into_owned();
        let mut source = Session::default();
        source.execute("solid.box", &json!({"length":10,"width":10,"height":10})).unwrap();
        source.execute("appearance.assign", &json!({"bodies":["Body1"],"name":name,"color":[31,61,91],"opacity":1.0})).unwrap();
        let expected = source.doc.appearances.bodies["Body1"].clone();
        source.execute("library.export", &json!({"path":path})).unwrap();
        h.app.run("solid.box", json!({"length":10,"width":10,"height":10})).unwrap();
        h.app.run("solid.box", json!({"length":5,"width":5,"height":5,"corner":[20,0,0]})).unwrap();
        h.app.session.selection = vec![Sel::Body { name: "Body1".into() }];
        h.app.start("appearance.assign");
        let Kind::Appearance(k) = &mut h.app.dialog.as_mut().unwrap().kind else { panic!("appearance editor") };
        k.local.path = path;
        h.frames(3);
        h.step(&json!({"widget":"Load library"})).unwrap();
        h.step(&json!({"click":{"handle":"library:entry:0"}})).unwrap();
        h.step(&json!({"widget":"Apply library appearance"})).unwrap();
        assert_eq!(h.app.session.doc.appearances.bodies["Body1"], expected);
        let undo = h.app.session.undo.len();
        h.step(&json!({"widget":"OK"})).unwrap();
        assert_eq!(h.app.session.undo.len(), undo);
        h.app.session.selection = vec![Sel::Body { name: "Body1".into() }];
        h.app.start("appearance.assign");
        h.frames(3);
        h.step(&json!({"widget":"OK"})).unwrap();
        assert!(h.app.dialog.is_none());
        assert_eq!(h.app.session.doc.appearances.bodies["Body1"], expected);
        h.app.session.selection = vec![Sel::Body { name: "Body1".into() }];
        h.app.start("appearance.assign");
        h.app.dialog.as_mut().unwrap().inputs[0].items = vec![Sel::Body { name: "Body2".into() }];
        h.frames(3);
        h.step(&json!({"widget":"OK"})).unwrap();
        assert_eq!(h.app.session.doc.appearances.bodies["Body2"], expected);
        assert!(solidcraft_engine::product_scene::extract(&h.app.session).unwrap().meshes.iter().all(|mesh| mesh.surface.is_none()));
    }
}

#[test]
fn catalog_clicked_load_apply_export_and_ok_keep_one_design_undo_step() {
    clicked_catalog(None, "solidcraft-library/1").unwrap();
}

#[test]
fn catalog_clicked_pbr_load_apply_export_and_ok_retains_the_optical_response() {
    clicked_catalog(Some(solidcraft_engine::doc::pbr_appearance::PbrParameters::new(1., 0.3, 1.5).unwrap()), "solidcraft-library/2").unwrap();
}

#[test]
fn catalog_clicked_rich_entry_without_pbr_clears_the_old_optical_response() {
    clicked_catalog(None, "solidcraft-library/2").unwrap();
}

fn clicked_catalog(pbr: Option<solidcraft_engine::doc::pbr_appearance::PbrParameters>, format: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1600,"height":1400}));
    let path = h.dir.join("source-library.json").to_string_lossy().into_owned();
    let mut source = Session::default();
    source.execute("solid.box", &json!({"length":10,"width":10,"height":10}))?;
    source.execute(
        "appearance.assign",
        &json!({"bodies":["Body1"],"appearance":"Warm oak colour","color":[180,120,70],"opacity":if pbr.is_some(){1.0}else{0.8},"pbr":pbr}),
    )?;
    source.execute("library.export", &json!({"path":path,"format":format}))?;
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10}))?;
    h.app.run("appearance.assign", json!({"bodies":["Body1"],"color":[20,20,20],"pbr":{"metallic":1.0,"roughness":0.6,"dielectric_ior":1.7}}))?;
    h.app.session.mark_saved();
    h.app.session.selection = vec![Sel::Body { name: "Body1".into() }];
    h.app.start("appearance.assign");
    if let Some(dialog) = &mut h.app.dialog
        && let Kind::Appearance(state) = &mut dialog.kind
    {
        state.local.path = path;
    }
    h.frames(3);
    let undo = h.app.session.undo.len();
    h.step(&json!({"widget":"Load library"}))?;
    for handle in [
        "library:path",
        "library:load",
        "library:search",
        "library:entry:0",
        "library:apply",
        "library:export",
        "dialog:button::Open library...",
        "dialog:button::Load library",
        "dialog:button::Apply library appearance",
        "dialog:button::Export design library...",
    ] {
        assert!(crate::scenario::handle_at(handle).is_some(), "missing {handle}");
    }
    for message in
        ["Replace existing library file", "Apply your appearance first. Exports assigned appearances and materials created in this design."]
    {
        assert!(h.widget(message).is_some(), "missing {message}");
    }
    h.step(&json!({"click":{"handle":"library:search"}}))?;
    h.step(&json!({"text":"no such appearance"}))?;
    assert!(h.widget("No matching appearance presets.").is_some());
    h.step(&json!({"key":"A","cmd":true}))?;
    h.step(&json!({"key":"Backspace"}))?;
    h.step(&json!({"click":{"handle":"library:entry:0"}}))?;
    h.step(&json!({"widget":"Apply library appearance"}))?;
    assert_eq!(h.app.session.doc.appearances.bodies["Body1"].name, "Warm oak colour");
    assert_eq!(h.app.session.doc.appearances.bodies["Body1"].pbr, pbr);
    if let Some(dialog) = &h.app.dialog
        && let Kind::Appearance(state) = &dialog.kind
    {
        assert_eq!(state.pbr, pbr);
    }

    assert_eq!(h.app.session.undo.len(), undo + 1);
    let revision = h.app.session.revision;
    h.step(&json!({"widget":"Export design library..."}))?;
    let exported = h.dir.join("my-library.solidcraft-library.json");
    let catalog = Catalog::read(&exported.to_string_lossy())?;
    assert_eq!(catalog.entries.len(), 1);
    assert_eq!(catalog.entries[0].definition.name(), "Warm oak colour");
    if let Definition::Appearance(look) = &catalog.entries[0].definition {
        assert_eq!(look.pbr, pbr);
    }
    assert_eq!(h.app.session.revision, revision);
    h.step(&json!({"widget":"OK"}))?;
    assert!(h.app.dialog.is_none());
    assert_eq!(h.app.session.undo.len(), undo + 1);
    Ok(())
}

fn appearance_fixture() -> Result<(Harness, Arc<()>), String> {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10}))?;
    h.app.run("appearance.assign", json!({"bodies":["Body1"],"appearance":"Warm oak colour","color":[180,120,70]}))?;
    h.app.session.mark_saved();
    let token = start_appearance(&mut h)?;
    Ok((h, token))
}

fn start_appearance(h: &mut Harness) -> Result<Arc<()>, String> {
    h.app.start("appearance.assign");
    match h.app.dialog.as_ref().map(|dialog| &dialog.kind) {
        Some(Kind::Appearance(k)) => Ok(k.local.token.clone()),
        _ => Err("appearance dialog missing".into()),
    }
}

#[test]
fn async_library_export_holds_its_document_until_the_answer() {
    use crate::picker_test_support::{Slot, save};
    let (mut h, token) = appearance_fixture().unwrap();
    let original = h.app.session.doc.clone();
    let path = h.dir.join("delayed-library.json");
    let ctx = crate::fonts::test_context();
    let slot: Slot = Default::default();
    save(&mut h.app, &slot);
    save_library(&mut h.app, token, false);
    crate::file_picker::tick(&mut h.app, &ctx);
    assert!(!path.exists());
    crate::documents::new_design(&mut h.app);
    let other = h.app.session.doc.clone();
    *slot.borrow_mut() = Some(Some(path.to_string_lossy().into_owned()));
    crate::file_picker::tick(&mut h.app, &ctx);
    assert_eq!(h.app.docs.active, 0, "upstream holds tab changes while a picker is pending");
    assert!(Arc::ptr_eq(&other, &h.app.session.doc));
    let catalog = Catalog::read(&path.to_string_lossy()).unwrap();
    assert_eq!(catalog.entries.len(), 1);
    assert_eq!(catalog.entries[0].definition.name(), "Warm oak colour");
    crate::documents::switch(&mut h.app, 0);
    assert!(Arc::ptr_eq(&original, &h.app.session.doc));
    assert!(!h.app.session.is_dirty());
}

#[test]
fn async_library_export_captures_overwrite_consent_and_cancel_writes_nothing() {
    use crate::picker_test_support::{Slot, save};
    let (mut h, token) = appearance_fixture().unwrap();
    let path = h.dir.join("existing-library.json");
    std::fs::write(&path, b"keep this file").unwrap();
    let original = h.app.session.doc.clone();
    let ctx = crate::fonts::test_context();
    let slot: Slot = Default::default();
    save(&mut h.app, &slot);
    save_library(&mut h.app, token.clone(), false);
    crate::file_picker::tick(&mut h.app, &ctx);
    editor(&mut h.app, &token).unwrap().local.overwrite = true;
    *slot.borrow_mut() = Some(Some(path.to_string_lossy().into_owned()));
    crate::file_picker::tick(&mut h.app, &ctx);
    assert_eq!(std::fs::read(&path).unwrap(), b"keep this file");
    assert!(matches!(&h.app.status, Some((_, _, true))));
    save_library(&mut h.app, token, false);
    crate::file_picker::tick(&mut h.app, &ctx);
    *slot.borrow_mut() = Some(None);
    crate::file_picker::tick(&mut h.app, &ctx);
    assert!(!h.app.file_picker_busy());
    assert_eq!(std::fs::read(&path).unwrap(), b"keep this file");
    assert!(Arc::ptr_eq(&original, &h.app.session.doc));
}

#[test]
fn async_library_open_does_not_replace_a_new_appearance_editor() {
    use crate::picker_test_support::{Slot, save};
    let (mut h, token) = appearance_fixture().unwrap();
    let path = h.dir.join("source-library.json");
    h.app.run("library.export", json!({"path":path})).unwrap();
    let original = h.app.session.doc.clone();
    let ctx = crate::fonts::test_context();
    let slot: Slot = Default::default();
    save(&mut h.app, &slot);
    // Prepare a different editor before the picker holds user commands, then model
    // an internal host replacing it while the old asynchronous answer is pending.
    let original_editor = h.app.dialog.take();
    let newer = start_appearance(&mut h).unwrap();
    editor(&mut h.app, &newer).unwrap().local.path = "new editor draft".into();
    let replacement_editor = h.app.dialog.take();
    h.app.dialog = original_editor;
    crate::picker_test_support::open(&mut h.app, &slot);
    open_library(&mut h.app, token);
    crate::file_picker::tick(&mut h.app, &ctx);
    h.app.dialog = replacement_editor;
    *slot.borrow_mut() = Some(Some(path.to_string_lossy().into_owned()));
    crate::file_picker::tick(&mut h.app, &ctx);
    let k = editor(&mut h.app, &newer).unwrap();
    assert_eq!(k.local.path, "new editor draft");
    assert!(k.local.catalog.is_none());
    assert!(Arc::ptr_eq(&original, &h.app.session.doc));
}

#[test]
fn delayed_library_export_refuses_an_internally_changed_design_without_overwriting() {
    use crate::picker_test_support::{Slot, save};
    let (mut h, token) = appearance_fixture().unwrap();
    let path = h.dir.join("keep.json");
    std::fs::write(&path, b"keep library").unwrap();
    let slot: Slot = Default::default();
    save(&mut h.app, &slot);
    save_library(&mut h.app, token, true);
    h.app.session.execute("appearance.assign", &json!({"bodies":["Body1"],"color":[1,2,3]})).unwrap();
    let changed = h.app.session.doc.clone();
    let history = (h.app.session.undo.len(), h.app.session.revision);
    *slot.borrow_mut() = Some(Some(path.to_string_lossy().into_owned()));
    crate::file_picker::tick(&mut h.app, &crate::fonts::test_context());
    assert_eq!(std::fs::read(path).unwrap(), b"keep library");
    assert!(Arc::ptr_eq(&changed, &h.app.session.doc));
    assert_eq!((h.app.session.undo.len(), h.app.session.revision), history);
    assert!(matches!(&h.app.status, Some((_, _, true))));
}

#[test]
fn catalog_clicked_material_import_renames_conflicts_and_closing_never_assigns_bodies() {
    for close in ["Cancel", "OK"] {
        let mut h = Harness::new();
        h.call("ui.resize", json!({"width":1600,"height":1600}));
        let path = h.dir.join("source-materials.json").to_string_lossy().into_owned();
        let mut source = Session::default();
        source
            .execute(
                "material.create",
                &json!({"name":"Library substrate","density_g_cm3":0.8,"color":[180,120,70],
            "pbr":{"metallic":0.0,"roughness":0.65,"dielectric_ior":1.4},
            "physical_card":{"schema_version":1,"designation":"Original illustrative substrate","family":"other",
                "sources":[{"id":"test","title":"Original source","kind":"illustrative","license":"MIT","reference":"fixture:original"}],
                "properties":[{"source_id":"test","conditions":{"temperature_k":293.15,"state":"synthetic"},
                    "data":{"kind":"density","kg_m3":700.0}}]}}),
            )
            .unwrap();
        source.execute("library.export", &json!({"path":path})).unwrap();
        let original = source.doc.user_materials[0].clone();
        h.app.run("solid.box", json!({"length":10,"width":10,"height":10})).unwrap();
        h.app.run("material.create", json!({"name":"Library substrate","density_g_cm3":1.2})).unwrap();
        h.app.run("material.assign", json!({"bodies":["Body1"],"material":"Library substrate"})).unwrap();
        let before = h.app.session.doc.clone();
        let mass = h.app.run("component.properties", json!({})).unwrap();
        h.app.session.selection = vec![Sel::Body { name: "Body1".into() }];
        h.app.start("appearance.assign");
        if let Some(dialog) = &mut h.app.dialog
            && let Kind::Appearance(state) = &mut dialog.kind
        {
            state.local.path = path.clone();
        }
        h.frames(3);
        h.step(&json!({"widget":"Load library"})).unwrap();
        h.step(&json!({"click":{"handle":"library:materials"}})).unwrap();
        h.step(&json!({"click":{"handle":"library:entry:0"}})).unwrap();
        let history = (h.app.session.undo.len(), h.app.session.revision);
        h.step(&json!({"click":{"handle":"library:material-import"}})).unwrap();
        assert_eq!(h.app.session.doc, before, "conflicting name must not mutate the design");
        assert_eq!((h.app.session.undo.len(), h.app.session.revision), history);
        h.step(&json!({"click":{"handle":"library:material-name"}})).unwrap();
        h.step(&json!({"key":"A","cmd":true})).unwrap();
        h.step(&json!({"text":"Copied substrate"})).unwrap();
        h.step(&json!({"click":{"handle":"library:material-import"}})).unwrap();
        let mut expected = original;
        expected.name = "Copied substrate".into();
        assert_eq!(h.app.session.doc.user_material("Copied substrate"), Some(&expected));
        assert_eq!(h.app.session.doc.appearances, before.appearances);
        assert_eq!(h.app.session.doc.materials, before.materials);
        assert_eq!(h.app.run("component.properties", json!({})).unwrap(), mass);
        assert_eq!(h.app.session.undo.len(), history.0 + 1);
        h.step(&json!({"widget":close})).unwrap();
        assert!(h.app.dialog.is_none());
        assert_eq!(h.app.session.undo.len(), history.0 + 1);
        assert_eq!(h.app.session.doc.appearances, before.appearances);
        assert_eq!(h.app.session.doc.materials, before.materials);
        let saved = solidcraft_engine::io::write_design_container(&h.app.session.doc).unwrap();
        std::fs::remove_file(path).unwrap();
        h.app.run("edit.undo", json!({})).unwrap();
        assert!(h.app.session.doc.user_material("Copied substrate").is_none());
        h.app.run("edit.redo", json!({})).unwrap();
        let reopened = solidcraft_engine::io::read_design(&saved).unwrap();
        assert_eq!(reopened.user_material("Copied substrate"), Some(&expected));
        assert_eq!(reopened.user_materials, h.app.session.doc.user_materials);
    }
}
