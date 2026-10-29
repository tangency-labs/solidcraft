use super::*;
use solidcraft_io::catalog::{Definition, Entry, FORMAT, LibraryLook, LibraryMaterial};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

struct File(String);
impl File {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        Self(
            solidcraft_io::vfs::temp_dir()
                .join(format!("solidcraft-catalog-{}-{}.json", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)))
                .to_string_lossy()
                .into_owned(),
        )
    }
    fn write(&self, catalog: &Catalog) {
        std::fs::write(&self.0, catalog.encode().unwrap()).unwrap();
    }
}
impl Drop for File {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
fn catalog() -> Catalog {
    Catalog {
        format: FORMAT.into(),
        id: "user.test".into(),
        name: "Original scalar test library".into(),
        entries: vec![
            Entry::new(
                "user.test",
                Definition::Appearance(LibraryLook { name: "Warm oak colour".into(), color: [180, 120, 70], opacity: 0.8, pbr: None }),
                Provenance::default(),
            )
            .unwrap(),
            Entry::new(
                "user.test",
                Definition::Material(LibraryMaterial {
                    name: "Timber density".into(),
                    density_g_cm3: 0.7,
                    physical_card: None,
                    appearance: Some(LibraryLook { name: "Timber tint".into(), color: [180, 120, 70], opacity: 1.0, pbr: None }),
                }),
                Provenance::default(),
            )
            .unwrap(),
        ],
    }
}
fn session() -> Session {
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length":10,"width":10,"height":10})).unwrap();
    s.mark_saved();
    s
}
fn apply_entry(s: &mut Session, file: &File, entry: &Entry, targets: Value) -> Result<Value> {
    s.execute("library.apply", &json!({"path":file.0,"entry":entry.id,"revision":entry.revision,"targets":targets}))
}

#[test]
fn catalog_world_instance_targets_map_once_and_invalid_targets_are_atomic() {
    let mut s = Session::default();
    let first = s.execute("component.create", &json!({"name":"Placed part"})).unwrap()["occurrence"].as_u64().unwrap();
    s.execute("solid.box", &json!({"length":2,"width":2,"height":2})).unwrap();
    let second = s.execute("occurrence.copy", &json!({"occurrence":first,"translate":[20,0,0]})).unwrap()["occurrence"].as_u64().unwrap();
    let instance = s.occurrence_bodies(&[second], false).unwrap()[0].clone();
    let source = s.occurrence_placement(&[second]).unwrap().bodies[0].source.clone();
    let file = File::new();
    let c = catalog();
    file.write(&c);
    let before = s.doc.clone();
    let history = (s.history.len(), s.history.redo_steps().len(), s.revision);
    for entry in &c.entries {
        assert!(apply_entry(&mut s, &file, entry, json!({"bodies":[instance,"missing"]})).is_err());
        assert!(Arc::ptr_eq(&before, &s.doc));
        assert_eq!((s.history.len(), s.history.redo_steps().len(), s.revision), history);
    }
    assert!(apply_entry(&mut s, &file, &c.entries[0], json!({"bodies":[instance],"faces":[{"body":instance,"point":[999,999,999]}]})).is_err());
    assert!(Arc::ptr_eq(&before, &s.doc));
    assert_eq!((s.history.len(), s.history.redo_steps().len(), s.revision), history);
    apply_entry(&mut s, &file, &c.entries[0], json!({"bodies":[instance],"faces":[{"body":instance,"point":[21,1,2]}]})).unwrap();
    let Definition::Appearance(look) = &c.entries[0].definition else { panic!("appearance fixture") };
    assert_eq!(s.doc.appearances.bodies[&source], look.to_look());
    let face = &s.doc.appearances.faces[0];
    assert_eq!(face.body, source);
    assert!(face.point.dist(solidcraft_geom::Vec3::new(1., 1., 2.)) < 1e-9);
    assert_eq!(face.look, look.to_look());
    assert_eq!(s.history.len(), history.0 + 1);
    let world = s.world_state();
    let placed = world.body(&instance).unwrap();
    assert!(s.doc.face_colors(placed).iter().any(|(_, assigned)| assigned == &look.to_look()));
    apply_entry(&mut s, &file, &c.entries[1], json!({"bodies":[instance]})).unwrap();
    assert_eq!(s.doc.materials.get(&source).map(String::as_str), Some("Timber density"));
    assert_eq!(s.history.len(), history.0 + 2);
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(s.doc.user_materials.is_empty());
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc, before);
}

#[test]
fn catalog_export_obeys_file_scope_in_new_file_and_overwrite_modes() {
    let file = File::new();
    let base = std::path::Path::new(&file.0).with_extension("scope");
    std::fs::create_dir(&base).unwrap();
    let root = base.join("allowed");
    let outside = base.join("outside");
    std::fs::create_dir(&root).unwrap();
    std::fs::create_dir(&outside).unwrap();
    let sentinel = outside.join("existing.json");
    std::fs::write(&sentinel, b"previous contents").unwrap();
    let mut s = session();
    let before = s.doc.clone();
    let undo = s.history.len();
    solidcraft_io::vfs::scoped(Some(solidcraft_io::vfs::PathScope::new([root.clone()])), || {
        for path in [outside.join("new.json"), root.join("../outside/traversal.json"), sentinel.clone()] {
            for overwrite in [false, true] {
                let error = s.execute("library.export", &json!({"path":path,"overwrite":overwrite})).unwrap_err();
                assert!(error.to_string().contains("outside the folders this agent may use"), "{error}");
            }
        }
        for overwrite in [false, true] {
            s.execute("library.export", &json!({"path":root.join("allowed.json"),"overwrite":overwrite})).unwrap();
        }
    });
    assert_eq!(std::fs::read(&sentinel).unwrap(), b"previous contents");
    assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 1);
    assert!(root.join("allowed.json").is_file());
    assert!(Arc::ptr_eq(&before, &s.doc));
    assert_eq!(s.history.len(), undo);
    assert!(!s.is_dirty());
    std::fs::remove_dir_all(&base).unwrap();
}

#[test]
fn catalog_appearance_copy_keeps_mass_and_survives_undo_and_cold_reopen_without_file() {
    let file = File::new();
    let catalog = catalog();
    file.write(&catalog);
    let mut s = session();
    let original_format = s.doc.format.clone();
    let before = s.execute("component.properties", &json!({})).unwrap()["total"]["mass_g"].as_f64().unwrap();
    let undo = s.history.len();
    apply_entry(&mut s, &file, &catalog.entries[0], json!({"bodies":["Body1"]})).unwrap();
    assert_eq!(s.history.len(), undo + 1);
    assert_eq!(s.doc.appearances.bodies["Body1"].name, "Warm oak colour");
    assert_eq!(s.execute("component.properties", &json!({})).unwrap()["total"]["mass_g"].as_f64().unwrap(), before);
    let saved = solidcraft_io::write_design(&s.doc);
    std::fs::remove_file(&file.0).unwrap();
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(s.doc.appearances.bodies.is_empty());
    s.execute("edit.redo", &json!({})).unwrap();
    let reopened = Session::new(solidcraft_io::read_design(&saved).unwrap());
    assert_eq!(reopened.doc.appearances, s.doc.appearances);
    assert_eq!(reopened.doc.format, original_format, "scalar library adds no document schema");
}

#[test]
fn catalog_physical_copy_preserves_exact_look_label_and_is_one_transaction() {
    let file = File::new();
    let catalog = catalog();
    file.write(&catalog);
    let mut s = session();
    let undo = s.history.len();
    apply_entry(&mut s, &file, &catalog.entries[1], json!({"bodies":["Body1"]})).unwrap();
    assert_eq!(s.history.len(), undo + 1);
    assert_eq!(s.doc.user_materials[0].appearance.as_ref().unwrap().name, "Timber tint");
    let mass = s.execute("component.properties", &json!({})).unwrap()["total"]["mass_g"].as_f64().unwrap();
    assert!((mass - 0.7).abs() < 1e-8);
    let saved = solidcraft_io::write_design(&s.doc);
    std::fs::remove_file(&file.0).unwrap();
    let mut reopened = Session::new(solidcraft_io::read_design(&saved).unwrap());
    let actual = reopened.execute("component.properties", &json!({})).unwrap()["total"]["mass_g"].as_f64().unwrap();
    assert!((actual - 0.7).abs() < 1e-8);
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(s.doc.user_materials.is_empty());
    assert!(s.doc.materials.is_empty());
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(s.doc.user_materials, reopened.doc.user_materials);
}

#[test]
fn catalog_conflicts_stale_revisions_and_invalid_targets_are_atomic() {
    let file = File::new();
    let catalog = catalog();
    file.write(&catalog);
    let mut s = session();
    s.execute("material.create", &json!({"name":"Timber density","density_g_cm3":1.2})).unwrap();
    s.execute("edit.undo", &json!({})).unwrap();
    let redo = s.history.redo_steps().len();
    let doc = s.doc.clone();
    let undo = s.history.len();
    let revision = s.revision;
    assert!(apply_entry(&mut s, &file, &catalog.entries[1], json!({"bodies":["missing"]})).is_err());
    assert_eq!(*s.doc, *doc);
    assert_eq!((s.history.len(), s.history.redo_steps().len(), s.revision), (undo, redo, revision));
    s.execute("edit.redo", &json!({})).unwrap();
    let doc = s.doc.clone();
    let history = (s.history.len(), s.history.redo_steps().len(), s.revision);
    assert!(apply_entry(&mut s, &file, &catalog.entries[1], json!({"bodies":["Body1"]})).is_err());
    assert_eq!(*s.doc, *doc);
    assert_eq!((s.history.len(), s.history.redo_steps().len(), s.revision), history);
    let mut changed = catalog.clone();
    if let Definition::Appearance(look) = &mut changed.entries[0].definition {
        look.color = [1, 2, 3];
    }
    changed.entries[0].revision = changed.entries[0].content_revision().unwrap();
    file.write(&changed);
    assert!(apply_entry(&mut s, &file, &catalog.entries[0], json!({"bodies":["Body1"]})).is_err());
    assert_eq!(*s.doc, *doc);
    assert_eq!((s.history.len(), s.history.redo_steps().len(), s.revision), history);
}

#[test]
fn catalog_explicit_material_rename_and_identical_definition_reuse() {
    let file = File::new();
    let catalog = catalog();
    file.write(&catalog);
    let entry = &catalog.entries[1];
    let mut s = session();
    s.execute("material.create", &json!({"name":"Timber density","density_g_cm3":1.2})).unwrap();
    let params = json!({"path":file.0,"entry":entry.id,"revision":entry.revision,"name":"Imported timber"});
    s.execute("library.apply", &params).unwrap();
    assert_eq!(s.doc.user_materials.len(), 2);
    let undo = s.history.len();
    let revision = s.revision;
    let again = s.execute("library.apply", &params).unwrap();
    assert_eq!(again["result"]["reused"], true);
    assert_eq!((s.history.len(), s.revision), (undo, revision));
    assert_eq!(s.doc.user_materials[0].density, 1.2);
}

#[test]
fn catalog_inspection_export_and_no_clobber_do_not_touch_design_or_history() {
    let file = File::new();
    let mut s = session();
    s.execute("appearance.assign", &json!({"bodies":["Body1"],"appearance":"Warm oak colour","color":[180,120,70],"opacity":0.8})).unwrap();
    s.mark_saved();
    let doc = s.doc.clone();
    let undo = s.history.len();
    let revision = s.revision;
    let result = s.execute("library.export", &json!({"path":file.0})).unwrap();
    assert_eq!(result["entries"], 1);
    assert!(Arc::ptr_eq(&doc, &s.doc));
    assert_eq!((s.history.len(), s.revision), (undo, revision));
    assert!(!s.is_dirty());
    let before = std::fs::read(&file.0).unwrap();
    s.execute("library.inspect", &json!({"path":file.0})).unwrap();
    assert!(s.execute("library.export", &json!({"path":file.0})).is_err());
    assert_eq!(before, std::fs::read(&file.0).unwrap());
    assert!(Arc::ptr_eq(&doc, &s.doc));
    assert_eq!((s.history.len(), s.revision), (undo, revision));
    s.execute("library.export", &json!({"path":file.0,"overwrite":true})).unwrap();
    assert_eq!(before, std::fs::read(&file.0).unwrap());
    for command in ["library.inspect", "library.export", "library.apply"] {
        assert!(s.execute(command, &json!({"path":file.0,"unknown":true})).is_err());
    }
}

#[test]
fn lossy_scalar_export_never_creates_or_replaces_a_file_or_changes_history() {
    let mut s = session();
    s.execute(
        "appearance.assign",
        &json!({"bodies":["Body1"],"color":[180,120,70],
        "pbr":{"metallic":1.0,"roughness":0.3,"dielectric_ior":1.5}}),
    )
    .unwrap();
    s.mark_saved();
    let doc = s.doc.clone();
    let history = (s.history.len(), s.history.redo_steps().len(), s.revision);
    for existing in [false, true] {
        let file = File::new();
        if existing {
            std::fs::write(&file.0, b"preserve existing library").unwrap();
        }
        for overwrite in [false, true] {
            let error =
                s.execute("library.export", &json!({"path":file.0,"overwrite":overwrite,"format":solidcraft_io::catalog::V1_FORMAT})).unwrap_err();
            assert!(error.to_string().contains("cannot preserve PBR"));
            if existing {
                assert_eq!(std::fs::read(&file.0).unwrap(), b"preserve existing library");
            } else {
                assert!(!std::path::Path::new(&file.0).exists());
            }
            assert!(Arc::ptr_eq(&doc, &s.doc));
            assert_eq!((s.history.len(), s.history.redo_steps().len(), s.revision), history);
            assert!(!s.is_dirty());
        }
    }
}

fn rich_catalog() -> Catalog {
    use solidcraft_doc::{pbr_appearance::PbrParameters, physical_material::ValidatedCard};
    let mut c = catalog();
    if let Definition::Appearance(l) = &mut c.entries[0].definition {
        l.opacity = 1.0;
        l.pbr = Some(PbrParameters::new(1., 0.25, 1.5).unwrap());
    }
    if let Definition::Material(m) = &mut c.entries[1].definition {
        m.density_g_cm3 = 0.8;
        m.appearance.as_mut().unwrap().pbr = Some(PbrParameters::new(0., 0.65, 1.4).unwrap());
        m.physical_card = Some(
            ValidatedCard::from_value(&json!({"schema_version":1,"designation":"Original synthetic candidate","family":"other",
            "sources":[{"id":"fixture","title":"Original invented density","kind":"illustrative","license":"MIT","reference":"fixture:original"},
                {"id":"unused","title":"Unused metadata retained","kind":"user_supplied","license":null,"reference":null}],
            "properties":[{"source_id":"fixture","conditions":{"temperature_k":293.15123456789,"moisture_dry_basis_kg_kg":0.1234567890123456,
                "strain_rate_per_s":0.0012345678901234567,"state":" Original synthetic fixture "},"data":{"kind":"density","kg_m3":700.0}}]}))
            .unwrap(),
        );
    }
    for e in &mut c.entries {
        *e = Entry::new("user.test", e.definition.clone(), e.provenance.clone()).unwrap();
    }
    c
}

#[test]
fn rich_appearance_copy_preserves_product_response_and_mass_without_source_file() {
    let file = File::new();
    let c = rich_catalog();
    file.write(&c);
    let mut s = session();
    let mass = s.execute("component.properties", &json!({})).unwrap();
    let scene = crate::product_scene::extract(&s).unwrap();
    let undo = s.history.len();
    apply_entry(&mut s, &file, &c.entries[0], json!({"bodies":["Body1"]})).unwrap();
    assert_eq!(s.history.len(), undo + 1);
    let after = crate::product_scene::extract(&s).unwrap();
    assert!(Arc::ptr_eq(&scene.meshes[0].mesh, &after.meshes[0].mesh));
    assert_eq!(after.meshes[0].surface.unwrap().metallic(), 1.0);
    assert_eq!(s.execute("component.properties", &json!({})).unwrap(), mass);
    let saved = solidcraft_io::write_design(&s.doc);
    std::fs::remove_file(&file.0).unwrap();
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(s.doc.appearances.bodies.is_empty());
    s.execute("edit.redo", &json!({})).unwrap();
    let mut reopened = Session::new(solidcraft_io::read_design(&saved).unwrap());
    assert_eq!(reopened.doc.appearances, s.doc.appearances);
    assert_eq!(reopened.execute("component.properties", &json!({})).unwrap(), mass);
}

#[test]
fn rich_material_copy_retains_engineering_card_label_optics_and_explicit_mass_density() {
    let file = File::new();
    let c = rich_catalog();
    file.write(&c);
    let mut s = session();
    let undo = s.history.len();
    apply_entry(&mut s, &file, &c.entries[1], json!({"bodies":["Body1"]})).unwrap();
    assert_eq!(s.history.len(), undo + 1);
    let material = &s.doc.user_materials[0];
    assert_eq!(material.appearance.as_ref().unwrap().name, "Timber tint");
    assert_eq!(material.appearance.as_ref().unwrap().pbr.unwrap().roughness(), 0.65);
    assert!(material.physical_card.is_some());
    let Definition::Material(source) = &c.entries[1].definition else { panic!("material fixture") };
    assert_eq!(material.physical_card, source.physical_card);
    assert_eq!(material.density, 0.8);
    let mass = s.execute("component.properties", &json!({})).unwrap()["total"]["mass_g"].as_f64().unwrap();
    assert!((mass - 0.8).abs() < 1e-8);
    let saved = solidcraft_io::write_design(&s.doc);
    std::fs::remove_file(&file.0).unwrap();
    let reopened = Session::new(solidcraft_io::read_design(&saved).unwrap());
    assert_eq!(reopened.doc.user_materials, s.doc.user_materials);
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(s.doc.user_materials.is_empty());
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(s.doc.user_materials, reopened.doc.user_materials);
}

#[test]
fn explicit_v1_export_refuses_richer_data_before_overwriting_and_scalar_v1_still_works() {
    let file = File::new();
    let sentinel = b"Existing catalog must survive";
    std::fs::write(&file.0, sentinel).unwrap();
    let mut s = session();
    s.execute(
        "appearance.assign",
        &json!({"bodies":["Body1"],"color":"#b47846",
        "pbr":{"metallic":1,"roughness":0.3,"dielectric_ior":1.5}}),
    )
    .unwrap();
    let before = s.doc.clone();
    let history = (s.history.len(), s.history.redo_steps().len(), s.revision);
    assert!(s.execute("library.export", &json!({"path":file.0,"format":solidcraft_io::catalog::V1_FORMAT,"overwrite":true})).is_err());
    assert_eq!(std::fs::read(&file.0).unwrap(), sentinel);
    assert!(Arc::ptr_eq(&before, &s.doc));
    assert_eq!(history, (s.history.len(), s.history.redo_steps().len(), s.revision));
    s.execute("library.export", &json!({"path":file.0,"overwrite":true})).unwrap();
    assert_eq!(Catalog::read(&file.0).unwrap().format, FORMAT);
    s.execute("appearance.assign", &json!({"bodies":["Body1"],"color":"#b47846"})).unwrap();
    s.execute("library.export", &json!({"path":file.0,"format":solidcraft_io::catalog::V1_FORMAT,"overwrite":true})).unwrap();
    assert_eq!(Catalog::read(&file.0).unwrap().format, solidcraft_io::catalog::V1_FORMAT);
    let rich = rich_catalog();
    file.write(&rich);
    s.execute("material.create", &json!({"name":"Timber density","density_g_cm3":0.8,"color":"#b47846"})).unwrap();
    let before = s.doc.clone();
    let history = (s.history.len(), s.history.redo_steps().len(), s.revision);
    assert!(apply_entry(&mut s, &file, &rich.entries[1], json!({"bodies":["Body1"]})).is_err());
    assert!(Arc::ptr_eq(&before, &s.doc));
    assert_eq!(history, (s.history.len(), s.history.redo_steps().len(), s.revision));
}

#[test]
fn catalog_labels_matching_starter_ids_are_inert_and_scalar_copies_clear_pbr_on_all_targets() {
    for format in [solidcraft_io::catalog::V1_FORMAT, FORMAT] {
        let mut s = Session::default();
        let component = s.execute("component.create", &json!({"name":"Part"})).unwrap()["component"].as_u64().unwrap();
        s.execute("solid.box", &json!({"body_name":"Specimen","length":10,"width":10,"height":10})).unwrap();
        let state = s.model.state();
        let bounds = state.body("Specimen").unwrap().mesh().bounds();
        let point = [(bounds.min.x + bounds.max.x) * 0.5, (bounds.min.y + bounds.max.y) * 0.5, bounds.max.z];
        let targets = json!({"bodies":["Specimen"],"faces":[{"body":"Specimen","point":point}],"components":["Part"]});
        let mut initial = targets.clone();
        initial["appearance"] = json!("Metal - Polished brass");
        initial["color"] = json!([20, 20, 20]);
        initial["pbr"] = json!({"metallic":1.0,"roughness":0.3,"dielectric_ior":1.5});
        s.execute("appearance.assign", &initial).unwrap();
        let file = File::new();
        let mut c = catalog().in_format(format).unwrap();
        let Definition::Appearance(look) = &mut c.entries[0].definition else { panic!("appearance fixture") };
        // This label must neither select nor bypass the unavailable glass recipe.
        look.name = "starter.glass.clear-soda-lime".into();
        let expected = look.to_look();
        c.entries[0] = Entry::new("user.test", c.entries[0].definition.clone(), c.entries[0].provenance.clone()).unwrap();
        file.write(&c);
        let before = s.doc.clone();
        let history = s.history.len();
        apply_entry(&mut s, &file, &c.entries[0], targets).unwrap();
        assert_eq!(s.doc.appearances.bodies["Specimen"], expected);
        assert_eq!(s.doc.appearances.components[&component], expected);
        assert_eq!(s.doc.appearances.faces[0].look, expected);
        assert_eq!(s.history.len(), history + 1);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc.appearances, before.appearances);
        s.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(s.doc.appearances.bodies["Specimen"], expected);
    }
}

#[test]
fn physical_catalog_import_respects_design_cap_and_builtin_names_without_partial_mutation() {
    let file = File::new();
    let c = rich_catalog();
    file.write(&c);
    let mut s = session();
    for i in 0..solidcraft_doc::MAX_USER_MATERIALS {
        s.doc_mut().user_materials.push(solidcraft_doc::UserMaterial {
            name: format!("Existing {i}"),
            density: 1.0,
            appearance: None,
            physical_card: None,
        });
    }
    let before = s.doc.clone();
    let history = (s.history.len(), s.history.redo_steps().len(), s.revision);
    assert!(apply_entry(&mut s, &file, &c.entries[1], json!({"bodies":["Body1"]})).is_err());
    assert_eq!(s.doc, before);
    assert_eq!((s.history.len(), s.history.redo_steps().len(), s.revision), history);
    let mut s = session();
    let before = s.doc.clone();
    let history = (s.history.len(), s.history.redo_steps().len(), s.revision);
    let entry = &c.entries[1];
    assert!(s.execute("library.apply", &json!({"path":file.0,"entry":entry.id,"revision":entry.revision,"name":"Steel"})).is_err());
    assert_eq!(s.doc, before);
    assert_eq!((s.history.len(), s.history.redo_steps().len(), s.revision), history);
}
