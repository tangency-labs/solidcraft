use crate::{Session, doc::Document, product_scene};
use serde_json::{Value, json};
use std::sync::Arc;

fn card() -> Value {
    json!({"schema_version":1,"designation":"Original synthetic card, not design data","family":"other",
        "sources":[{"id":"fixture","title":"Original analytic constants","kind":"illustrative","license":"MIT OR Apache-2.0"}],
        "properties":[{"source_id":"fixture","conditions":{"temperature_k":293.15,"state":"synthetic"},
            "data":{"kind":"density","kg_m3":700.0}},
            {"source_id":"fixture","data":{"kind":"elasticity","model":{
                "symmetry":"isotropic","young_modulus_pa":12.25e9,"poisson_ratio":0.25}}}]})
}

fn fixture() -> Session {
    let mut s = Session::default();
    s.execute("solid.box", &json!({"body_name":"Specimen","length":10,"width":20,"height":5})).unwrap();
    s.execute(
        "material.create",
        &json!({"name":"Synthetic","density_g_cm3":0.8,"color":"#b47846",
        "pbr":{"metallic":0,"roughness":0.4,"dielectric_ior":1.5}}),
    )
    .unwrap();
    s.execute("material.assign", &json!({"bodies":["Specimen"],"material":"Synthetic"})).unwrap();
    s
}

#[test]
fn card_edits_preserve_mass_optics_and_geometry_and_survive_undo_reopen() {
    let mut s = fixture();
    let scene = product_scene::extract(&s).unwrap();
    let mass = s.execute("component.properties", &json!({})).unwrap();
    let before_undo = s.undo.len();
    let out = s.execute("material.edit", &json!({"material":"Synthetic","physical_card":card()})).unwrap();
    assert_eq!(out["physical_validation"]["isotropic_moduli"]["shear_pa"], 4.9e9);
    assert_eq!(out["density_comparison"]["assigned_density_kg_m3"], 800.0);
    assert_eq!(out["density_comparison"]["matches_within_relative_tolerance"], false);
    assert_eq!(out["physical_validation"]["source_review_required"], json!(["fixture"]));
    assert_eq!(out["physical_validation"]["analysis_solver_available"], false);
    assert_eq!(s.undo.len(), before_undo + 1);
    assert_eq!(s.execute("component.properties", &json!({})).unwrap(), mass);
    let after = product_scene::extract(&s).unwrap();
    assert!(Arc::ptr_eq(&scene.meshes[0].mesh, &after.meshes[0].mesh));
    assert_eq!(scene.meshes[0].surface, after.meshes[0].surface);
    let saved = s.doc.to_json();
    assert_eq!(serde_json::from_str::<Value>(&saved).unwrap()["format"], solidcraft_doc::format::format_string(solidcraft_doc::format::FORMAT));
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(s.doc.user_materials[0].physical_card.is_none());
    s.execute("edit.redo", &json!({})).unwrap();
    let mut reopened = Session::new(Document::from_json(&saved).unwrap());
    assert_eq!(reopened.execute("component.properties", &json!({})).unwrap(), mass);
    assert_eq!(reopened.doc.user_materials, s.doc.user_materials);
    let modulus = reopened.doc.user_materials[0].physical_card.as_ref().unwrap().card().directional_modulus([1., 2., 3.]).unwrap();
    assert_eq!(modulus.young_modulus_pa, 12.25e9);
    let doc = reopened.doc.clone();
    let before = (reopened.revision, reopened.undo.len(), reopened.camera, reopened.selection.clone());
    let list = reopened.execute("material.list", &json!({})).unwrap();
    let material = list["materials"].as_array().unwrap().iter().find(|m| m["name"] == "Synthetic").unwrap();
    assert_eq!(material["physical_card"], out["physical_card"]);
    assert!(Arc::ptr_eq(&doc, &reopened.doc));
    assert_eq!(before, (reopened.revision, reopened.undo.len(), reopened.camera, reopened.selection.clone()));
}

#[test]
fn omission_retains_null_clears_and_unrelated_material_edits_retain_cards() {
    let mut s = fixture();
    s.execute("material.create", &json!({"name":"With card","density_g_cm3":0.7,"physical_card":card()})).unwrap();
    let original = s.doc.user_materials[1].physical_card.clone();
    for p in [json!({"density_g_cm3":0.9}), json!({"color":"#112233"}), json!({"name":"Renamed"})] {
        let mut p = p;
        p["material"] = json!(s.doc.user_materials[1].name);
        s.execute("material.edit", &p).unwrap();
        assert_eq!(s.doc.user_materials[1].physical_card, original);
    }
    let mut only_card = Document::default();
    only_card.user_materials.push(s.doc.user_materials[1].clone());
    let error = solidcraft_io::catalog::Catalog::from_document(&only_card, "user.fixture", "Original", Default::default())
        .and_then(|c| c.in_format(solidcraft_io::catalog::V1_FORMAT))
        .unwrap_err();
    assert!(error.to_string().contains("engineering cards"));
    s.execute("material.edit", &json!({"material":"Renamed","physical_card":null})).unwrap();
    assert!(s.doc.user_materials[1].physical_card.is_none());
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc.user_materials[1].physical_card, original);
}

#[test]
fn hostile_cards_refuse_atomically_and_saved_card_admission_is_not_bypassed() {
    let mut s = fixture();
    let mut bad = card();
    bad["properties"][1]["data"]["model"]["poisson_ratio"] = json!(0.6);
    let mut oversized = card();
    oversized["sources"] = json!(vec![json!({}); 33]);
    let mut unknown = card();
    unknown["unrecognized"] = json!(true);
    let mut missing = card();
    missing["properties"][0]["source_id"] = json!("missing");
    let doc = s.doc.clone();
    let before = (s.undo.len(), s.redo.len(), s.revision, s.camera, s.selection.clone());
    for candidate in [bad, oversized, unknown, missing, json!(true)] {
        assert!(s.execute("material.edit", &json!({"material":"Synthetic","physical_card":candidate})).is_err());
        assert!(s.execute("material.create", &json!({"name":"Refused","density_g_cm3":1,"physical_card":candidate})).is_err());
        assert!(Arc::ptr_eq(&doc, &s.doc));
        assert_eq!(before, (s.undo.len(), s.redo.len(), s.revision, s.camera, s.selection.clone()));
        let mut saved = serde_json::to_value(&*s.doc).unwrap();
        saved["user_materials"][0]["physical_card"] = candidate;
        assert!(Document::from_json(&saved.to_string()).is_err());
        assert!(serde_json::from_value::<Document>(saved).is_err());
    }
    // Large finite SI constants are accepted only in this typed card, not geometry.
    let mut saved = serde_json::to_value(&*s.doc).unwrap();
    saved["user_materials"][0]["physical_card"] = card();
    assert!(Document::from_json(&saved.to_string()).is_ok());
    saved["hostile_numeric_probe"] = json!(12.25e9);
    assert!(Document::from_json(&saved.to_string()).unwrap_err().to_string().contains("number"));
}

#[test]
fn configurations_rename_and_paste_new_keep_design_owned_material_semantics() {
    let mut s = Session::default();
    let part = s.execute("component.create", &json!({"name":"Part"})).unwrap()["component"].as_u64().unwrap();
    s.execute("solid.box", &json!({"body_name":"Specimen","length":10,"width":10,"height":10})).unwrap();
    s.execute("material.create", &json!({"name":"Synthetic","density_g_cm3":0.8,"physical_card":card()})).unwrap();
    s.execute("config.configure", &json!({"materials":["Specimen"]})).unwrap();
    s.execute("config.row", &json!({"name":"Candidate","values":{"material:Specimen":"Synthetic"}})).unwrap();
    s.execute("config.activate", &json!({"row":"Candidate"})).unwrap();
    s.execute("component.activate", &json!({"component":0})).unwrap();
    s.execute("component.paste_new", &json!({"component":part,"translate":[30,0,0]})).unwrap();
    assert_eq!(s.doc.user_materials.len(), 1);
    assert_eq!(s.doc.materials.values().filter(|m| *m == "Synthetic").count(), 2);
    let original = s.doc.user_materials[0].physical_card.clone();
    s.execute("material.edit", &json!({"material":"Synthetic","name":"Renamed candidate"})).unwrap();
    s.execute("config.activate", &json!({"row":"Default"})).unwrap();
    s.execute("config.activate", &json!({"row":"Candidate"})).unwrap();
    let loaded = Document::from_json(&s.doc.to_json()).unwrap();
    assert_eq!(loaded.user_materials[0].physical_card, original);
    assert_eq!(loaded.materials.values().filter(|m| *m == "Renamed candidate").count(), 2);
    assert!(loaded.configs.rows.iter().any(|r| r.cells.iter().any(|c| c == "Renamed candidate")));
}

#[test]
fn format21_materials_without_cards_keep_their_optical_look() {
    let s = fixture();
    let mut saved = serde_json::to_value(&*s.doc).unwrap();
    saved["format"] = json!("solidcraft/21");
    let loaded = Document::from_json(&saved.to_string()).unwrap();
    assert_eq!(loaded.user_materials, s.doc.user_materials);
    assert!(loaded.user_materials[0].physical_card.is_none());
    assert_eq!(loaded.format, solidcraft_doc::format::format_string(solidcraft_doc::format::FORMAT));
}

#[test]
fn scalar_catalog_refuses_card_export_without_creating_or_replacing_files_or_history() {
    struct ProbeFile(std::path::PathBuf);
    impl Drop for ProbeFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let unique = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let file = ProbeFile(std::env::temp_dir().join(format!("solidcraft-card-refusal-{}-{unique}.json", std::process::id())));
    let mut s = Session::default();
    s.execute("material.create", &json!({"name":"Card only","density_g_cm3":0.8,"physical_card":card()})).unwrap();
    s.mark_saved();
    let doc = s.doc.clone();
    let history = (s.undo.len(), s.redo.len(), s.revision);
    for existing in [false, true] {
        if existing {
            std::fs::write(&file.0, b"keep existing catalog").unwrap();
        }
        for overwrite in [false, true] {
            // The default writer is rich v2. Request scalar v1 explicitly so this
            // regression still exercises refusal rather than a valid lossless copy.
            let error =
                s.execute("library.export", &json!({"path":file.0,"overwrite":overwrite,"format":solidcraft_io::catalog::V1_FORMAT})).unwrap_err();
            let message = error.to_string();
            assert!(message.contains("catalog v1") && message.contains("engineering cards"), "{message}");
            if existing {
                assert_eq!(std::fs::read(&file.0).unwrap(), b"keep existing catalog");
            } else {
                assert!(!file.0.exists());
            }
            assert!(Arc::ptr_eq(&doc, &s.doc));
            assert_eq!((s.undo.len(), s.redo.len(), s.revision), history);
            assert!(!s.is_dirty());
        }
    }
}

#[test]
fn native_container_keeps_large_si_card_values_sources_conditions_and_density_separate() {
    let mut s = fixture();
    s.execute("material.edit", &json!({"material":"Synthetic","physical_card":card()})).unwrap();
    let before = s.execute("component.properties", &json!({})).unwrap();
    let scene = product_scene::extract(&s).unwrap();
    let native = solidcraft_io::write_design_container(&s.doc).unwrap();
    assert!(solidcraft_io::is_design_container(&native));
    let loaded = solidcraft_io::read_design(&native).unwrap();
    assert_eq!(loaded.format, solidcraft_doc::format::format_string(solidcraft_doc::format::FORMAT));
    assert_eq!(loaded.user_materials, s.doc.user_materials);
    let mut reopened = Session::new(loaded);
    assert_eq!(reopened.execute("component.properties", &json!({})).unwrap(), before);
    assert_eq!(product_scene::extract(&reopened).unwrap().meshes[0].surface, scene.meshes[0].surface);
    let list = reopened.execute("material.list", &json!({})).unwrap();
    let material = list["materials"].as_array().unwrap().iter().find(|m| m["name"] == "Synthetic").unwrap();
    assert_eq!(material["physical_card"], serde_json::to_value(s.doc.user_materials[0].physical_card.as_ref().unwrap()).unwrap());
    assert_eq!(material["density_comparison"]["assigned_density_kg_m3"], 800.0);
    assert_eq!(material["density_comparison"]["card_density_kg_m3"], 700.0);
    assert_eq!(material["physical_validation"]["source_review_required"], json!(["fixture"]));
}

#[test]
fn format20_materials_without_cards_upgrade_to21_without_changing_their_optical_look() {
    let s = fixture();
    let mut saved = serde_json::to_value(&*s.doc).unwrap();
    saved["format"] = json!("solidcraft/20");
    let loaded = Document::from_json(&saved.to_string()).unwrap();
    assert_eq!(loaded.user_materials, s.doc.user_materials);
    assert!(loaded.user_materials[0].physical_card.is_none());
    assert_eq!(loaded.format, solidcraft_doc::format::format_string(solidcraft_doc::format::FORMAT));
}

#[test]
fn card_number_admission_preserves_metadata_without_exempting_other_material_fields() {
    let s = fixture();
    let mut saved = serde_json::to_value(&*s.doc).unwrap();
    saved["user_materials"][0]["physical_card"] = card();
    let metadata = json!({"org.example.measurement":{"value":12.25e9,"physical_card":{"untyped":12.25e9}}});
    saved["metadata"] = metadata.clone();
    let loaded = Document::from_json(&saved.to_string()).unwrap();
    let reopened = solidcraft_io::read_design(&solidcraft_io::write_design_container(&loaded).unwrap()).unwrap();
    assert_eq!(serde_json::to_value(&reopened.metadata).unwrap(), metadata);
    assert_eq!(reopened.user_materials, loaded.user_materials);

    // Only the exact root user-material card receives SI admission. The same key
    // nested elsewhere must not exempt a hostile number from geometry limits.
    for field in ["appearance", "unrecognized"] {
        let mut hostile = saved.clone();
        hostile["user_materials"][0][field] = json!({"physical_card":{"untyped":12.25e9}});
        assert!(Document::from_json(&hostile.to_string()).unwrap_err().to_string().contains("number"));
    }
    saved["user_materials"][0]["density"] = json!(12.25e9);
    assert!(Document::from_json(&saved.to_string()).unwrap_err().to_string().contains("number"));
}
