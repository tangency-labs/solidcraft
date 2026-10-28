use serde_json::json;
use std::sync::Arc;

use crate::{Session, product_scene};

fn fixture() -> Session {
    let mut s = Session::default();
    s.execute("solid.box", &json!({"body_name":"Specimen","length":10,"width":20,"height":5})).unwrap();
    s
}

#[test]
fn copied_recipe_id_labels_are_inert_and_explicit_recipe_requests_stay_strict() {
    let mut s = fixture();
    for name in ["starter.metal.gold-warm", "starter.glass.clear-soda-lime"] {
        for pbr in [serde_json::Value::Null, json!({"metallic":0.0,"roughness":0.73,"dielectric_ior":1.37})] {
            s.execute("appearance.assign", &json!({"bodies":["Specimen"],"name":name,"color":[31,61,91],"opacity":1.0,"pbr":pbr})).unwrap();
            let look = &s.doc.appearances.bodies["Specimen"];
            assert_eq!(look.name, name);
            assert_eq!(look.color, [31, 61, 91]);
            assert_eq!(serde_json::to_value(look.pbr).unwrap(), pbr);
            assert_eq!(product_scene::extract(&s).unwrap().meshes[0].surface.is_some(), !pbr.is_null());
        }
    }
    let before = s.doc.clone();
    let history = (s.undo.len(), s.redo.len(), s.revision);
    for params in [
        json!({"bodies":["Specimen"],"name":"Owned label"}),
        json!({"bodies":["Specimen"],"name":"Owned label","appearance":"starter.metal.gold-warm","color":[31,61,91]}),
        json!({"bodies":["Specimen"],"appearance":"starter.glass.clear-soda-lime","color":[31,61,91],"pbr":null}),
        json!({"bodies":["Specimen"],"name":"x".repeat(129),"color":[31,61,91]}),
    ] {
        assert!(s.execute("appearance.assign", &params).is_err());
        assert!(Arc::ptr_eq(&before, &s.doc));
        assert_eq!((s.undo.len(), s.redo.len(), s.revision), history);
    }
    s.execute("appearance.assign", &json!({"bodies":["Specimen"],"appearance":"starter.metal.gold-warm"})).unwrap();
    assert_eq!(s.doc.appearances.bodies["Specimen"].name, "Metal - Warm Reflective Gold");
    assert_eq!(s.doc.appearances.bodies["Specimen"].pbr.unwrap().metallic(), 1.0);
}

#[test]
fn format20_scalar_looks_upgrade_and_format21_setups_keep_their_meaning() {
    let mut s = fixture();
    s.execute("appearance.assign", &json!({"bodies":["Specimen"],"appearance":"Original legacy look","color":"#b47846","opacity":0.4})).unwrap();
    let before = serde_json::to_value(&s.doc.appearances).unwrap();
    let mut legacy = serde_json::to_value(&*s.doc).unwrap();
    legacy["format"] = json!("solidcraft/20");
    legacy.as_object_mut().unwrap().remove("render");
    let upgraded = crate::doc::Document::from_json(&legacy.to_string()).unwrap();
    assert_eq!(upgraded.format, crate::doc::format::format_string(crate::doc::format::FORMAT));
    assert_eq!(serde_json::to_value(&upgraded.appearances).unwrap(), before);
    assert!(upgraded.render.setups.is_empty());
    s.execute("render.settings.set", &json!({"name":"Studio","set":{"width":320,"height":240}})).unwrap();
    let mut saved: serde_json::Value = serde_json::from_str(&s.doc.to_json()).unwrap();
    saved["format"] = json!("solidcraft/21");
    let d = crate::doc::Document::from_json(&saved.to_string()).unwrap();
    assert_eq!(d.format, crate::doc::format::format_string(crate::doc::format::FORMAT));
    assert_eq!(serde_json::to_value(&d.appearances).unwrap(), before);
    assert!(d.appearances.bodies["Specimen"].pbr.is_none());
    assert_eq!(d.render.setups["Studio"].width, 320);
    for version in [crate::doc::format::FORMAT + 1, crate::doc::format::FORMAT + 2] {
        saved["format"] = json!(format!("solidcraft/{version}"));
        assert!(crate::doc::Document::from_json(&saved.to_string()).is_err());
    }
}

#[test]
fn opaque_response_preserves_geometry_mass_undo_and_cold_reopening() {
    let mut s = fixture();
    let before = product_scene::extract(&s).unwrap();
    let mass = s.execute("component.properties", &json!({})).unwrap();
    s.execute(
        "appearance.assign",
        &json!({"bodies":["Specimen"],"color":"#b47846","appearance":"Original opaque test",
        "pbr":{"metallic":1.0,"roughness":0.3,"dielectric_ior":1.5}}),
    )
    .unwrap();
    let after = product_scene::extract(&s).unwrap();
    assert!(Arc::ptr_eq(&before.meshes[0].mesh, &after.meshes[0].mesh));
    assert_eq!(s.execute("component.properties", &json!({})).unwrap(), mass);
    assert!(after.meshes[0].surface.is_some());
    assert_eq!(after.required_features(), vec![crate::render::RenderFeature::Pbr]);
    let saved = s.doc.to_json();
    assert_eq!(serde_json::from_str::<serde_json::Value>(&saved).unwrap()["format"], crate::doc::format::format_string(crate::doc::format::FORMAT));
    let look = &s.doc.appearances.bodies["Specimen"];
    assert_eq!(crate::doc::config::appearance_cell(&serde_json::to_string(look).unwrap()).unwrap().as_ref(), Some(look));
    let list = s.execute("appearance.list", &json!({})).unwrap();
    assert_eq!(list["in_design"][0]["pbr"]["metallic"], 1.0);
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(product_scene::extract(&s).unwrap().meshes[0].surface.is_none());
    s.execute("edit.redo", &json!({})).unwrap();
    assert!(product_scene::extract(&s).unwrap().meshes[0].surface.is_some());
    let mut reopened = Session::new(crate::doc::Document::from_json(&saved).unwrap());
    assert_eq!(product_scene::extract(&reopened).unwrap().meshes[0].surface, after.meshes[0].surface);
    assert_eq!(reopened.execute("component.properties", &json!({})).unwrap(), mass);
}

#[test]
fn invalid_optical_requests_refuse_atomically_and_scalar_export_cannot_lose_them() {
    let mut s = fixture();
    let doc = s.doc.clone();
    let history = (s.undo.len(), s.redo.len(), s.revision);
    for params in [
        json!({"bodies":["Specimen"],"pbr":{"metallic":0,"roughness":0.5,"dielectric_ior":1.5}}),
        json!({"bodies":["Specimen"],"color":"#ffffff","pbr":{"metallic":0,"roughness":0,"dielectric_ior":1.5}}),
        json!({"bodies":["Specimen"],"color":"#ffffff","opacity":0.5,"pbr":{"metallic":0,"roughness":0.5,"dielectric_ior":1.5}}),
        json!({"bodies":["Specimen"],"appearance":"Named","color":"#ffffff","opacity":"invalid","pbr":{"metallic":0,"roughness":0.5,"dielectric_ior":1.5}}),
        json!({"bodies":["Specimen"],"appearance":"Named","color":"#ffffff","opacity":2,"pbr":{"metallic":0,"roughness":0.5,"dielectric_ior":1.5}}),
        json!({"bodies":["Specimen"],"color":"#ffffff","pbr":{"metallic":0,"roughness":0.5,"dielectric_ior":1.5,"transmission":1}}),
    ] {
        assert!(s.execute("appearance.assign", &params).is_err());
        assert!(Arc::ptr_eq(&doc, &s.doc));
        assert_eq!((s.undo.len(), s.redo.len(), s.revision), history);
    }
    s.execute(
        "appearance.assign",
        &json!({"bodies":["Specimen"],"color":"#ffffff",
        "pbr":{"metallic":0,"roughness":0.5,"dielectric_ior":1.5}}),
    )
    .unwrap();
    assert!(
        solidcraft_io::catalog::Catalog::from_document(&s.doc, "user.fixture", "Original", Default::default())
            .and_then(|c| c.in_format(solidcraft_io::catalog::V1_FORMAT))
            .is_err()
    );
    let mut invalid: serde_json::Value = serde_json::from_str(&s.doc.to_json()).unwrap();
    invalid["appearances"]["bodies"]["Specimen"]["opacity"] = json!(0.5);
    assert!(crate::doc::Document::from_json(&invalid.to_string()).is_err());
    invalid["appearances"]["bodies"]["Specimen"]["opacity"] = json!(1.0);
    invalid["appearances"]["bodies"]["Specimen"]["pbr"]["roughness"] = json!(0.0);
    assert!(crate::doc::Document::from_json(&invalid.to_string()).is_err());
}

#[test]
fn physical_default_look_and_face_override_keep_independent_optical_meaning() {
    let mut s = fixture();
    s.execute(
        "material.create",
        &json!({"name":"Original density fixture","density_g_cm3":0.7,"color":"#b47846",
        "pbr":{"metallic":0,"roughness":0.4,"dielectric_ior":1.5}}),
    )
    .unwrap();
    s.execute("material.assign", &json!({"bodies":["Specimen"],"material":"Original density fixture"})).unwrap();
    let scene = product_scene::extract(&s).unwrap();
    assert!(scene.meshes[0].surface.is_some());
    s.execute("appearance.assign", &json!({"faces":[[5,10,5]],"body":"Specimen","color":"#0080a0"})).unwrap();
    let scene = product_scene::extract(&s).unwrap();
    assert!(scene.meshes[0].surface.is_some());
    assert!(scene.meshes[0].faces.iter().any(|f| f.color == [0, 128, 160] && f.surface.is_none()));
    assert_eq!(s.doc.user_materials[0].density, 0.7);
    assert!(
        solidcraft_io::catalog::Catalog::from_document(&s.doc, "user.fixture", "Original", Default::default())
            .and_then(|c| c.in_format(solidcraft_io::catalog::V1_FORMAT))
            .is_err()
    );
}

#[test]
fn independent_component_copy_keeps_its_opaque_response_after_source_edits_and_reopen() {
    let mut s = Session::default();
    let part = s.execute("component.create", &json!({"name":"Part"})).unwrap()["component"].as_u64().unwrap();
    s.execute("solid.box", &json!({"body_name":"Specimen","length":10,"width":20,"height":5})).unwrap();
    s.execute(
        "appearance.assign",
        &json!({"components":["Part"],"color":"#b47846",
        "pbr":{"metallic":1,"roughness":0.3,"dielectric_ior":1.5}}),
    )
    .unwrap();
    s.execute("component.activate", &json!({"component":0})).unwrap();
    let copied = s.execute("component.paste_new", &json!({"component":part,"translate":[30,0,0]})).unwrap()["component"].as_u64().unwrap();
    let original = s.doc.appearances.components[&part].clone();
    assert_eq!(s.doc.appearances.components[&copied], original);
    s.execute(
        "appearance.assign",
        &json!({"components":["Part"],"color":"#b47846",
        "pbr":{"metallic":1,"roughness":0.8,"dielectric_ior":1.5}}),
    )
    .unwrap();
    assert_ne!(s.doc.appearances.components[&part], original);
    assert_eq!(s.doc.appearances.components[&copied], original);
    let reopened = crate::doc::Document::from_json(&s.doc.to_json()).unwrap();
    assert_eq!(reopened.appearances.components[&copied], original);
}

#[test]
fn named_opaque_finishes_reach_the_scene_and_survive_undo_and_reopen_without_changing_mass() {
    let mut s = fixture();
    s.execute("material.assign", &json!({"bodies":["Specimen"],"material":"Steel"})).unwrap();
    let mass = s.execute("component.properties", &json!({})).unwrap();
    let geometry = product_scene::extract(&s).unwrap().meshes[0].mesh.clone();
    let library = s.execute("appearance.library", &json!({})).unwrap();
    let entries = library["appearances"].as_array().unwrap();
    // Exact pre-finish compatibility fixture. Identify legacy rows by their
    // names/values, not by missing IDs: independent future packs may coexist.
    let legacy: [(&str, [u8; 3], f64); 20] = [
        ("Aluminum - Satin", [204, 208, 214], 1.0),
        ("Aluminum - Anodized Black", [44, 46, 50], 1.0),
        ("Aluminum - Anodized Blue", [52, 92, 160], 1.0),
        ("Steel - Satin", [158, 164, 172], 1.0),
        ("Stainless Steel - Brushed", [186, 191, 198], 1.0),
        ("Cast Iron", [104, 104, 110], 1.0),
        ("Brass - Polished", [208, 172, 88], 1.0),
        ("Copper", [204, 122, 80], 1.0),
        ("Titanium", [150, 148, 158], 1.0),
        ("ABS - White", [236, 236, 230], 1.0),
        ("ABS - Black", [36, 36, 38], 1.0),
        ("ABS - Red", [196, 40, 40], 1.0),
        ("ABS - Blue", [40, 90, 186], 1.0),
        ("ABS - Yellow", [236, 196, 40], 1.0),
        ("ABS - Green", [52, 150, 80], 1.0),
        ("Rubber - Black", [28, 28, 28], 1.0),
        ("Glass - Clear", [190, 220, 228], 0.25),
        ("Glass - Smoked", [90, 96, 104], 0.45),
        ("Wood - Oak", [190, 146, 92], 1.0),
        ("Paint - Gloss White", [246, 246, 244], 1.0),
    ];
    assert_eq!(solidcraft_doc::appearance::LIBRARY, legacy);
    for (name, color, opacity) in legacy {
        let matching: Vec<_> = entries.iter().filter(|entry| entry["name"] == name).collect();
        assert_eq!(matching.len(), 1, "legacy row must occur exactly once: {name}");
        let row = matching[0];
        assert_eq!(row["color"], format!("#{:02x}{:02x}{:02x}", color[0], color[1], color[2]));
        assert_eq!(row["opacity"], opacity);
        assert!(row.get("pbr").is_none() && row.get("id").is_none(), "legacy scalar meaning changed: {name}");
    }
    assert_eq!(
        solidcraft_doc::appearance::OPAQUE_FINISHES.map(|(name, ..)| name),
        ["Steel - Satin PBR", "Aluminum - Satin PBR", "Copper - Clean PBR", "ABS - Molded PBR", "PLA - Smooth PBR", "Wood - Oak Uniform PBR Preview"]
    );
    for (name, ..) in solidcraft_doc::appearance::OPAQUE_FINISHES {
        let expected = solidcraft_doc::appearance::library(name).unwrap();
        let matching: Vec<_> = entries.iter().filter(|entry| entry["name"] == name).collect();
        assert_eq!(matching.len(), 1, "finish row must occur exactly once: {name}");
        assert_eq!(matching[0]["color"], expected.hex());
        assert_eq!(matching[0]["opacity"], 1.0);
        assert_eq!(matching[0]["pbr"], json!(expected.pbr.unwrap()));
        assert!(!legacy.iter().any(|(legacy_name, ..)| legacy_name == &name));
        let before = s.doc.appearances.clone();
        s.execute("appearance.assign", &json!({"bodies":["Specimen"],"appearance":name.to_lowercase()})).unwrap();
        assert_eq!(s.doc.appearances.bodies["Specimen"], expected);
        let scene = product_scene::extract(&s).unwrap();
        assert_eq!(scene.meshes[0].surface, expected.opaque_surface().unwrap());
        assert!(Arc::ptr_eq(&scene.meshes[0].mesh, &geometry));
        assert_eq!(scene.required_features(), vec![crate::render::RenderFeature::Pbr]);
        assert_eq!(s.execute("component.properties", &json!({})).unwrap(), mass);
        let saved = s.doc.to_json();
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc.appearances, before);
        s.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(s.doc.appearances.bodies["Specimen"], expected);
        let mut reopened = Session::new(solidcraft_doc::Document::from_json(&saved).unwrap());
        assert_eq!(reopened.doc.appearances.bodies["Specimen"], expected);
        assert_eq!(reopened.execute("component.properties", &json!({})).unwrap(), mass);
        assert!(
            solidcraft_io::catalog::Catalog::from_document(&reopened.doc, "user.fixture", "Original", Default::default())
                .unwrap()
                .in_format(solidcraft_io::catalog::V1_FORMAT)
                .is_err()
        );
    }
}

#[test]
fn named_opaque_finish_refuses_transparency_atomically_and_allows_explicit_scalar_override() {
    let mut s = fixture();
    s.execute("appearance.assign", &json!({"bodies":["Specimen"],"color":"#123456"})).unwrap();
    s.execute("edit.undo", &json!({})).unwrap();
    let doc = s.doc.clone();
    let history = (s.undo.len(), s.redo.len(), s.revision);
    assert!(history.1 > 0);
    for opacity in [json!(0), json!(0.5), json!(-0.1), json!(2), json!("invalid"), json!(null), json!(true)] {
        assert!(s.execute("appearance.assign", &json!({"bodies":["Specimen"],"appearance":"Steel - Satin PBR","opacity":opacity})).is_err());
        assert!(Arc::ptr_eq(&doc, &s.doc));
        assert_eq!((s.undo.len(), s.redo.len(), s.revision), history);
    }
    s.execute("appearance.assign", &json!({"bodies":["Specimen"],"appearance":"Steel - Satin PBR","pbr":null,"opacity":0.5})).unwrap();
    let scalar = s.doc.appearances.bodies["Specimen"].clone();
    assert!(scalar.pbr.is_none());
    assert_eq!(scalar.opacity, 0.5);
    assert!(product_scene::extract(&s).unwrap().meshes[0].surface.is_none());
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc.appearances, doc.appearances);
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(s.doc.appearances.bodies["Specimen"], scalar);
    let reopened = crate::doc::Document::from_json(&s.doc.to_json()).unwrap();
    assert_eq!(reopened.appearances.bodies["Specimen"], scalar);
}

#[test]
fn physical_material_presets_refuse_inherited_transparency_before_any_edit() {
    let mut s = fixture();
    s.execute("material.create", &json!({"name":"Existing material","density_g_cm3":2,"color":"#123456"})).unwrap();
    s.execute("material.assign", &json!({"bodies":["Specimen"],"material":"Existing material"})).unwrap();
    s.execute("material.edit", &json!({"material":"Existing material","density_g_cm3":3})).unwrap();
    s.execute("edit.undo", &json!({})).unwrap();
    let mass = s.execute("component.properties", &json!({})).unwrap();
    let doc = s.doc.clone();
    let history = (s.undo.len(), s.redo.len(), s.revision);
    assert!(history.1 > 0);
    for command in ["material.create", "material.edit"] {
        let mut params = if command == "material.create" {
            json!({"name":"New material","density_g_cm3":4,"appearance":"Steel - Satin PBR"})
        } else {
            // A rejected optical edit must not rename the material, rewrite uses or change density.
            json!({"material":"Existing material","name":"Renamed material","density_g_cm3":4,"appearance":"Steel - Satin PBR"})
        };
        for opacity in [json!(0), json!(0.5), json!(-0.1), json!(2), json!("invalid"), json!(null), json!(true)] {
            params["opacity"] = opacity;
            assert!(s.execute(command, &params).is_err(), "{command}: {params}");
            assert!(Arc::ptr_eq(&doc, &s.doc));
            assert_eq!((s.undo.len(), s.redo.len(), s.revision), history);
        }
    }
    assert_eq!(s.execute("component.properties", &json!({})).unwrap(), mass);
}

#[test]
fn physical_material_finish_defaults_keep_optics_through_edits_undo_and_reopen() {
    let mut s = fixture();
    let expected = solidcraft_doc::appearance::library("ABS - Molded PBR").unwrap();
    s.execute("material.create", &json!({"name":"Original density fixture","density_g_cm3":0.7,"appearance":"ABS - Molded PBR"})).unwrap();
    assert_eq!(s.doc.user_materials[0].appearance.as_ref(), Some(&expected));
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(s.doc.user_materials.is_empty());
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(s.doc.user_materials[0].appearance.as_ref(), Some(&expected));
    s.execute("material.assign", &json!({"bodies":["Specimen"],"material":"Original density fixture"})).unwrap();
    let geometry = product_scene::extract(&s).unwrap().meshes[0].mesh.clone();
    assert_eq!(product_scene::extract(&s).unwrap().meshes[0].surface, expected.opaque_surface().unwrap());

    s.execute("material.edit", &json!({"material":"Original density fixture","density_g_cm3":0.9})).unwrap();
    assert_eq!(s.doc.user_materials[0].appearance.as_ref(), Some(&expected));
    assert_eq!(s.doc.density("Specimen"), 0.9);
    let reopened = Session::new(crate::doc::Document::from_json(&s.doc.to_json()).unwrap());
    assert_eq!(reopened.doc.user_materials[0].appearance.as_ref(), Some(&expected));
    assert_eq!(product_scene::extract(&reopened).unwrap().meshes[0].surface, expected.opaque_surface().unwrap());
    let mass = s.execute("component.properties", &json!({})).unwrap();
    s.execute("material.edit", &json!({"material":"Original density fixture","appearance":"Steel - Satin PBR","pbr":null,"opacity":0.4})).unwrap();
    let scalar = s.doc.user_materials[0].appearance.clone().unwrap();
    assert!(scalar.pbr.is_none());
    assert_eq!(scalar.opacity, 0.4);
    assert_eq!(scalar.color, [158, 164, 172]);
    let scene = product_scene::extract(&s).unwrap();
    assert!(scene.meshes[0].surface.is_none());
    assert!(Arc::ptr_eq(&scene.meshes[0].mesh, &geometry));
    assert_eq!(s.execute("component.properties", &json!({})).unwrap(), mass);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc.user_materials[0].appearance.as_ref(), Some(&expected));
    assert_eq!(product_scene::extract(&s).unwrap().meshes[0].surface, expected.opaque_surface().unwrap());
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(s.doc.user_materials[0].appearance.as_ref(), Some(&scalar));
    let mut reopened = Session::new(crate::doc::Document::from_json(&s.doc.to_json()).unwrap());
    assert_eq!(reopened.doc.user_materials, s.doc.user_materials);
    assert_eq!(reopened.execute("component.properties", &json!({})).unwrap(), mass);
    assert!(product_scene::extract(&reopened).unwrap().meshes[0].surface.is_none());

    s.execute(
        "material.create",
        &json!({"name":"Explicit scalar fixture","density_g_cm3":1.1,"appearance":"Copper - Clean PBR","pbr":null,"opacity":0.25}),
    )
    .unwrap();
    let created_scalar = s.doc.user_materials[1].appearance.as_ref().unwrap();
    assert!(created_scalar.pbr.is_none());
    assert_eq!(created_scalar.opacity, 0.25);
    assert_eq!(created_scalar.color, [204, 122, 80]);
}

#[test]
fn custom_looks_with_preset_names_keep_their_explicit_color_and_response() {
    let mut s = fixture();
    let mass = s.execute("component.properties", &json!({})).unwrap();
    // UI and catalog swatches pass both a name and a color; the name does not opt in to a preset.
    s.execute("appearance.assign", &json!({"bodies":["Specimen"],"appearance":"Steel - Satin PBR","color":"#123456","opacity":0.4})).unwrap();
    let scalar = s.doc.appearances.bodies["Specimen"].clone();
    assert_eq!(scalar.name, "Steel - Satin PBR");
    assert_eq!(scalar.color, [18, 52, 86]);
    assert_eq!(scalar.opacity, 0.4);
    assert!(scalar.pbr.is_none());
    assert!(product_scene::extract(&s).unwrap().meshes[0].surface.is_none());
    let response = json!({"metallic":0.25,"roughness":0.8,"dielectric_ior":1.8});
    s.execute("appearance.assign", &json!({"bodies":["Specimen"],"appearance":"Steel - Satin PBR","color":"#123456","pbr":response})).unwrap();
    let custom = s.doc.appearances.bodies["Specimen"].clone();
    assert_eq!(custom.color, scalar.color);
    assert_eq!(json!(custom.pbr.unwrap()), response);
    assert_ne!(custom, solidcraft_doc::appearance::library("Steel - Satin PBR").unwrap());
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc.appearances.bodies["Specimen"], scalar);
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(s.doc.appearances.bodies["Specimen"], custom);
    let reopened = crate::doc::Document::from_json(&s.doc.to_json()).unwrap();
    assert_eq!(reopened.appearances.bodies["Specimen"], custom);
    assert_eq!(s.execute("component.properties", &json!({})).unwrap(), mass);

    // A physical material's custom default may also have a name matching a built-in appearance.
    s.execute("material.create", &json!({"name":"Steel - Satin PBR","density_g_cm3":1.2,"color":"#123456","opacity":0.4})).unwrap();
    assert_eq!(s.doc.user_materials[0].appearance.as_ref(), Some(&scalar));
}

fn pack_session() -> Session {
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length":10,"width":20,"height":30,"body_name":"Swatch"})).unwrap();
    s.execute("material.assign", &json!({"bodies":["Swatch"],"material":"Aluminum"})).unwrap();
    s
}

#[test]
fn pack_listing_keeps_legacy_rows_and_labels_unavailable_recipes_without_mutation() {
    let mut s = pack_session();
    let doc = s.doc.clone();
    let undo = s.undo.len();
    let reply = s.execute("appearance.library", &json!({})).unwrap();
    let rows = reply["appearances"].as_array().unwrap();
    // Compatibility fixture: exact pre-pack scalar library, independent of the
    // separate six PBR finishes (#1366), which may also have no catalog IDs.
    let legacy: [(&str, [u8; 3], f64); 20] = [
        ("Aluminum - Satin", [204, 208, 214], 1.0),
        ("Aluminum - Anodized Black", [44, 46, 50], 1.0),
        ("Aluminum - Anodized Blue", [52, 92, 160], 1.0),
        ("Steel - Satin", [158, 164, 172], 1.0),
        ("Stainless Steel - Brushed", [186, 191, 198], 1.0),
        ("Cast Iron", [104, 104, 110], 1.0),
        ("Brass - Polished", [208, 172, 88], 1.0),
        ("Copper", [204, 122, 80], 1.0),
        ("Titanium", [150, 148, 158], 1.0),
        ("ABS - White", [236, 236, 230], 1.0),
        ("ABS - Black", [36, 36, 38], 1.0),
        ("ABS - Red", [196, 40, 40], 1.0),
        ("ABS - Blue", [40, 90, 186], 1.0),
        ("ABS - Yellow", [236, 196, 40], 1.0),
        ("ABS - Green", [52, 150, 80], 1.0),
        ("Rubber - Black", [28, 28, 28], 1.0),
        ("Glass - Clear", [190, 220, 228], 0.25),
        ("Glass - Smoked", [90, 96, 104], 0.45),
        ("Wood - Oak", [190, 146, 92], 1.0),
        ("Paint - Gloss White", [246, 246, 244], 1.0),
    ];
    assert_eq!(solidcraft_doc::appearance::LIBRARY, legacy);
    for (name, color, opacity) in legacy {
        let matching: Vec<_> = rows.iter().filter(|row| row["name"] == name).collect();
        assert_eq!(matching.len(), 1, "legacy appearance must occur exactly once: {name}");
        let row = matching[0];
        assert_eq!(row["color"], format!("#{:02x}{:02x}{:02x}", color[0], color[1], color[2]));
        assert_eq!(row["opacity"], opacity);
        assert!(row.get("id").is_none() && row.get("pbr").is_none(), "legacy scalar meaning changed: {name}");
    }
    let finishes: [(&str, [u8; 3], f64, f64); 6] = [
        ("Steel - Satin PBR", [158, 164, 172], 1.0, 0.4),
        ("Aluminum - Satin PBR", [204, 208, 214], 1.0, 0.3),
        ("Copper - Clean PBR", [204, 122, 80], 1.0, 0.25),
        ("ABS - Molded PBR", [222, 222, 216], 0.0, 0.4),
        ("PLA - Smooth PBR", [110, 160, 222], 0.0, 0.35),
        ("Wood - Oak Uniform PBR Preview", [190, 146, 92], 0.0, 0.65),
    ];
    assert_eq!(solidcraft_doc::appearance::OPAQUE_FINISHES, finishes);
    for (name, color, metallic, roughness) in finishes {
        let matching: Vec<_> = rows.iter().filter(|row| row["name"] == name).collect();
        assert_eq!(matching.len(), 1, "published finish must occur exactly once: {name}");
        let row = matching[0];
        assert_eq!(row["color"], format!("#{:02x}{:02x}{:02x}", color[0], color[1], color[2]));
        assert_eq!(row["opacity"], 1.0);
        assert_eq!(row["pbr"], json!({"metallic":metallic,"roughness":roughness,"dielectric_ior":1.5}));
        assert!(row.get("id").is_none());
    }
    assert_eq!(rows.len(), 66);
    assert_eq!(rows.iter().filter(|row| row["available"] != false).count(), 60);
    assert_eq!(rows.iter().filter(|row| row["available"] != false && row["opacity"] == 1.0).count(), 58);
    assert_eq!(rows.iter().filter(|row| row["opacity"] != 1.0).count(), 2);
    assert_eq!(rows.iter().filter(|v| v.get("id").is_some()).count(), 40);
    assert_eq!(rows.iter().filter(|v| v["available"] == false).count(), 6);
    for row in rows.iter().filter(|v| v["category"] == "glass") {
        assert_eq!(row["available"], false);
        assert_eq!(row["opacity"], 1.0);
        assert!(row["pbr"].is_null());
        assert_eq!(row["requirements"], json!(["refraction", "certified_thin_or_closed"]));
    }
    assert!(Arc::ptr_eq(&doc, &s.doc));
    assert_eq!(s.undo.len(), undo);
}

#[test]
fn opaque_pack_uses_normal_assignment_undo_and_preserves_engineering_properties() {
    let mut s = pack_session();
    let before = s.doc.clone();
    let undo = s.undo.len();
    let density = s.doc.density("Swatch");
    let physical = s.execute("component.properties", &json!({"component":"root"})).unwrap()["total"].clone();
    s.execute("appearance.assign", &json!({"bodies":["Swatch"],"appearance":"starter.metal.gold-warm"})).unwrap();
    let look = s.doc.appearances.bodies.get("Swatch").unwrap();
    assert_eq!(look.pbr.unwrap().metallic(), 1.0);
    assert_eq!(look.opacity, 1.0);
    assert_eq!(s.undo.len(), undo + 1);
    assert_eq!(s.doc.density("Swatch"), density);
    assert_eq!(s.execute("component.properties", &json!({"component":"root"})).unwrap()["total"], physical);
    let assigned = s.doc.clone();
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(*s.doc, *before);
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(*s.doc, *assigned);
}

#[test]
fn unavailable_glass_and_invalid_opaque_override_refuse_atomically() {
    let mut s = pack_session();
    let doc = s.doc.clone();
    let before = (s.revision, s.undo.len(), s.redo.len(), s.is_dirty());
    for extra in [json!({}), json!({"color":"#ffffff"}), json!({"opacity":0.25,"pbr":null})] {
        let mut params = json!({"bodies":["Swatch"],"appearance":"starter.glass.clear-soda-lime"});
        params.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        assert!(s.execute("appearance.assign", &params).unwrap_err().to_string().contains("refraction"));
        assert!(Arc::ptr_eq(&doc, &s.doc));
        assert_eq!(before, (s.revision, s.undo.len(), s.redo.len(), s.is_dirty()));
    }
    assert!(s.execute("appearance.assign", &json!({"bodies":["Swatch"],"appearance":"starter.wood.oak-honey","opacity":0.4})).is_err());
    assert!(Arc::ptr_eq(&doc, &s.doc));
    assert_eq!(before, (s.revision, s.undo.len(), s.redo.len(), s.is_dirty()));
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn cold_native_reopen_keeps_owned_opaque_values_without_pack_metadata_or_assets() {
    let mut s = pack_session();
    s.execute("appearance.assign", &json!({"bodies":["Swatch"],"appearance":"starter.wood.walnut-dark"})).unwrap();
    let look = s.doc.appearances.bodies.get("Swatch").unwrap().clone();
    let density = s.doc.density("Swatch");
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("solidcraft-pack-{}-{nonce}", std::process::id()));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("opaque.solidcraft");
    s.execute("file.save", &json!({"path":path.to_str().unwrap()})).unwrap();
    let mut reopened = Session::default();
    reopened.execute("doc.open", &json!({"path":path.to_str().unwrap()})).unwrap();
    assert_eq!(reopened.doc.appearances.bodies.get("Swatch"), Some(&look));
    assert_eq!(reopened.doc.density("Swatch"), density);
    let json = reopened.doc.to_json();
    assert!(!json.contains("starter.wood.walnut-dark") && !json.contains("starter_pack.rs"));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn starter_selectors_keep_final_validation_and_custom_names_stay_inert() {
    let mut s = pack_session();
    let preset = solidcraft_doc::appearance_pack::find("starter.wood.oak-honey").unwrap();
    let doc = s.doc.clone();
    let history = (s.undo.len(), s.redo.len(), s.revision);
    for selector in [preset.id, preset.name] {
        for opacity in [json!(0.5), json!(2.0), json!("1"), json!(null)] {
            assert!(s.execute("appearance.assign", &json!({"bodies":["Swatch"],"appearance":selector,"opacity":opacity})).is_err());
            assert!(Arc::ptr_eq(&doc, &s.doc));
            assert_eq!((s.undo.len(), s.redo.len(), s.revision), history);
        }
    }
    s.execute("appearance.assign", &json!({"bodies":["Swatch"],"appearance":preset.id,"pbr":null,"opacity":0.4})).unwrap();
    assert!(s.doc.appearances.bodies["Swatch"].pbr.is_none());
    assert_eq!(s.doc.appearances.bodies["Swatch"].opacity, 0.4);
    // A copied name is inert; it is not a request to select the registered recipe.
    s.execute("appearance.assign", &json!({"bodies":["Swatch"],"name":preset.name,"color":"#123456","opacity":0.6})).unwrap();
    let look = &s.doc.appearances.bodies["Swatch"];
    assert_eq!((look.name.as_str(), look.color, look.opacity, look.pbr), (preset.name, [18, 52, 86], 0.6, None));
    s.execute("appearance.assign", &json!({"bodies":["Swatch"],"appearance":preset.name})).unwrap();
    assert_eq!(s.doc.appearances.bodies["Swatch"], preset.opaque_look().unwrap());
}

#[test]
fn starter_visual_defaults_and_overrides_keep_saved_physical_cards_independent() {
    let mut s = pack_session();
    let card = json!({"schema_version":1,"designation":"Original illustrative pack fixture","family":"other",
        "sources":[{"id":"fixture","title":"Original analytic density","kind":"illustrative","license":"MIT OR Apache-2.0"}],
        "properties":[{"source_id":"fixture","data":{"kind":"density","kg_m3":750.0}}]});
    s.execute(
        "material.create",
        &json!({"name":"Pack substrate","density_g_cm3":0.8,"physical_card":card,
        "appearance":"starter.plastic.abs-charcoal"}),
    )
    .unwrap();
    s.execute("material.assign", &json!({"bodies":["Swatch"],"material":"Pack substrate"})).unwrap();
    // Input omission and canonical saved defaults are equivalent. Compare the full
    // validated representation, not the sparse input object, throughout the edits.
    let expected_card = serde_json::to_value(serde_json::from_value::<crate::doc::physical_material::ValidatedCard>(card.clone()).unwrap()).unwrap();
    let physical = s.execute("material.inspect", &json!({"material":"Pack substrate"})).unwrap();
    assert_eq!(physical["physical_card"], expected_card);
    let mass = s.execute("component.properties", &json!({})).unwrap();
    let geometry = product_scene::extract(&s).unwrap().meshes[0].mesh.clone();
    s.execute("appearance.assign", &json!({"bodies":["Swatch"],"appearance":"starter.metal.gold-warm"})).unwrap();
    assert_eq!(s.execute("material.inspect", &json!({"material":"Pack substrate"})).unwrap(), physical);
    assert_eq!(s.execute("component.properties", &json!({})).unwrap(), mass);
    assert!(Arc::ptr_eq(&geometry, &product_scene::extract(&s).unwrap().meshes[0].mesh));
    let before = s.doc.clone();
    let history = (s.undo.len(), s.redo.len(), s.revision);
    for command in ["material.create", "material.edit"] {
        let params = json!({"name":"Refused glass","material":"Pack substrate","density_g_cm3":1.0,
            "appearance":"starter.glass.clear-soda-lime","pbr":null,"opacity":0.25});
        assert!(s.execute(command, &params).is_err());
        assert!(Arc::ptr_eq(&before, &s.doc));
        assert_eq!((s.undo.len(), s.redo.len(), s.revision), history);
    }
    s.execute("material.edit", &json!({"material":"Pack substrate","appearance":"starter.wood.walnut-dark"})).unwrap();
    assert_eq!(s.execute("material.inspect", &json!({"material":"Pack substrate"})).unwrap()["physical_card"], expected_card);
    assert_eq!(s.execute("component.properties", &json!({})).unwrap(), mass);
    let saved = s.doc.to_json();
    let mut reopened = Session::new(crate::doc::Document::from_json(&saved).unwrap());
    assert_eq!(reopened.doc.user_materials, s.doc.user_materials);
    assert_eq!(reopened.execute("material.inspect", &json!({"material":"Pack substrate"})).unwrap()["physical_card"], expected_card);
    assert_eq!(reopened.execute("component.properties", &json!({})).unwrap(), mass);
}

#[test]
fn starter_names_and_ids_share_admission_before_all_overrides() {
    use crate::doc::appearance_pack::{Recipe, STARTER_PACK};
    let mut s = pack_session();
    for preset in &STARTER_PACK {
        for selector in [preset.id, preset.name] {
            if matches!(preset.recipe, Recipe::Glass(_)) {
                for overrides in [
                    json!({}),
                    json!({"color":"#aabbcc"}),
                    json!({"color":"#aabbcc","opacity":0.5,"pbr":null}),
                    json!({"color":"#aabbcc","pbr":{"metallic":0.0,"roughness":0.5,"dielectric_ior":1.5}}),
                ] {
                    let before = s.doc.clone();
                    let history = (s.undo.len(), s.redo.len(), s.revision);
                    let mut params = overrides;
                    params["bodies"] = json!(["Swatch"]);
                    params["appearance"] = json!(selector);
                    let error = s.execute("appearance.assign", &params).unwrap_err().to_string();
                    assert!(error.contains("refraction"), "{selector}: {error}");
                    assert!(Arc::ptr_eq(&before, &s.doc));
                    assert_eq!((s.undo.len(), s.redo.len(), s.revision), history);
                }
            } else {
                s.execute("appearance.assign", &json!({"bodies":["Swatch"],"appearance":selector,"color":[31,61,91]})).unwrap();
                let look = &s.doc.appearances.bodies["Swatch"];
                assert_eq!(look.name, preset.name);
                assert_eq!(look.color, [31, 61, 91]);
                assert_eq!(look.pbr, preset.opaque_look().unwrap().pbr);
            }
        }
    }
}

fn chain_read(s: &mut Session, target: serde_json::Value) -> serde_json::Value {
    s.execute("appearance.chain", &json!({"target":target})).unwrap()
}

#[test]
fn appearance_chain_and_from_material_preserve_mass_and_all_four_steps() {
    let mut s = Session::default();
    s.execute("component.create", &json!({"name":"Part"})).unwrap();
    s.execute("solid.box", &json!({"body_name":"Specimen","length":10,"width":20,"height":5})).unwrap();
    s.execute("material.create", &json!({"name":"Substrate","density_g_cm3":2.3,"appearance":"Steel - Satin PBR"})).unwrap();
    s.execute("material.assign", &json!({"bodies":["Specimen"],"material":"Substrate"})).unwrap();
    let mass = s.execute("component.properties", &json!({})).unwrap();
    let physical = s.doc.user_materials.clone();
    let face = s.execute("model.faces", &json!({"body":"Specimen"})).unwrap()["faces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["point"][2] == 5.0)
        .unwrap()["index"]
        .clone();
    let target = json!({"body":"Specimen","face":face});
    s.execute("appearance.assign", &json!({"components":["Part"],"appearance":"ABS - Blue"})).unwrap();
    s.execute("appearance.assign", &json!({"bodies":["Specimen"],"color":"#abcdef","opacity":0.5})).unwrap();
    s.execute("appearance.assign", &json!({"body":"Specimen","faces":[[5,10,5]],"appearance":"Copper - Clean PBR"})).unwrap();
    let history = (s.undo.len(), s.redo.len(), s.revision);
    let doc = s.doc.clone();
    let chain = chain_read(&mut s, target.clone());
    assert_eq!(chain["active"], "face");
    assert_eq!(
        chain["steps"].as_array().unwrap().iter().map(|s| s["step"].as_str().unwrap()).collect::<Vec<_>>(),
        ["face", "body", "component", "material"]
    );
    assert_eq!(chain["steps"][0]["look"]["name"], "Copper - Clean PBR");
    assert_eq!(chain["steps"][1]["look"]["opacity"], 0.5);
    assert_eq!(chain["steps"][2]["look"]["name"], "ABS - Blue");
    assert_eq!(chain["steps"][3]["look"]["pbr"]["metallic"], 1.0);
    assert!(Arc::ptr_eq(&doc, &s.doc));
    assert_eq!((s.undo.len(), s.redo.len(), s.revision), history);
    assert_eq!(chain_read(&mut s, json!({"body":"Specimen"}))["active"], "body");
    assert_eq!(chain_read(&mut s, json!({"component":"Part"}))["active"], "component");
    // The existing clear form walks down the same chain, one edit at a time.
    s.execute("appearance.assign", &json!({"body":"Specimen","faces":[[6,11,5]],"color":null})).unwrap();
    assert_eq!(chain_read(&mut s, target.clone())["active"], "body");
    s.execute("appearance.assign", &json!({"bodies":["Specimen"],"color":null})).unwrap();
    assert_eq!(chain_read(&mut s, target.clone())["active"], "component");
    s.execute("appearance.assign", &json!({"components":["Part"]})).unwrap();
    for t in [target.clone(), json!({"body":"Specimen"}), json!({"component":"Part"})] {
        assert_eq!(chain_read(&mut s, t)["active"], "material");
    }
    // Clearing each target independently also restores material when no lower override exists.
    for assign in [json!({"bodies":["Specimen"]}), json!({"components":["Part"]}), json!({"body":"Specimen","faces":[[5,10,5]]})] {
        let mut painted = assign.clone();
        painted["appearance"] = json!("ABS - Red");
        s.execute("appearance.assign", &painted).unwrap();
        s.execute("appearance.assign", &assign).unwrap();
        assert_eq!(chain_read(&mut s, target.clone())["active"], "material");
        assert_eq!(s.execute("component.properties", &json!({})).unwrap(), mass);
    }
    assert_eq!(s.doc.user_materials, physical);
    let state = s.model.state();
    let body = state.body("Specimen").unwrap();
    assert_eq!(s.doc.model_face_look(body, face.as_u64().unwrap() as usize), s.doc.model_display_look(body));
    let mut reopened = Session::new(crate::doc::Document::from_json(&s.doc.to_json()).unwrap());
    assert_eq!(chain_read(&mut reopened, target.clone()), chain_read(&mut s, target));
}

#[test]
fn appearance_replace_is_exact_atomic_one_undo_and_survives_reopen() {
    let mut s = Session::default();
    s.execute("component.create", &json!({"name":"Part"})).unwrap();
    s.execute("solid.box", &json!({"body_name":"Specimen","length":10,"width":20,"height":5})).unwrap();
    s.execute("material.create", &json!({"name":"Substrate","density_g_cm3":2.3,"appearance":"Steel - Satin PBR"})).unwrap();
    s.execute("material.assign", &json!({"bodies":["Specimen"],"material":"Substrate"})).unwrap();
    s.execute(
        "appearance.assign",
        &json!({"bodies":["Specimen"],"components":["Part"],"body":"Specimen","faces":[[5,10,5]],"appearance":"Steel - Satin PBR"}),
    )
    .unwrap();
    // Same label and color, different optical values: full equality must distinguish it.
    let mut scalar = s.doc.appearances.bodies["Specimen"].clone();
    scalar.pbr = None;
    s.doc_mut().appearances.bodies.insert("Suppressed fixture".into(), scalar.clone());
    let mass = s.execute("component.properties", &json!({})).unwrap();
    let scene = product_scene::extract(&s).unwrap();
    let before = s.doc.clone();
    let history = s.undo.len();
    let listed = s.execute("appearance.list", &json!({})).unwrap();
    let from = listed["in_design"].as_array().unwrap().iter().find(|l| l.get("pbr").is_some()).unwrap().clone();
    let params = json!({"from":from,"to":"Copper - Clean PBR"});
    assert_eq!(s.execute("appearance.replace", &params).unwrap()["count"], 3);
    assert_eq!(s.undo.len(), history + 1);
    assert_eq!(s.doc.appearances.bodies["Suppressed fixture"], scalar);
    assert_eq!(s.doc.user_materials, before.user_materials);
    assert_eq!(s.doc.materials, before.materials);
    assert_eq!(s.doc.features, before.features);
    assert_eq!(s.doc.occurrences, before.occurrences);
    assert_eq!(s.execute("component.properties", &json!({})).unwrap(), mass);
    let after_scene = product_scene::extract(&s).unwrap();
    assert!(Arc::ptr_eq(&scene.meshes[0].mesh, &after_scene.meshes[0].mesh));
    let after = s.doc.appearances.clone();
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc.appearances, before.appearances);
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(s.doc.appearances, after);
    let reopened = Session::new(crate::doc::Document::from_json(&s.doc.to_json()).unwrap());
    assert_eq!(reopened.doc.appearances, after);
    let snapshot = s.doc.clone();
    let history = (s.undo.len(), s.redo.len(), s.revision);
    assert_eq!(s.execute("appearance.replace", &params).unwrap()["count"], 0);
    assert_eq!(s.execute("appearance.replace", &json!({"from":"Copper - Clean PBR","to":"Copper - Clean PBR"})).unwrap()["count"], 0);
    assert!(Arc::ptr_eq(&snapshot, &s.doc));
    assert_eq!((s.undo.len(), s.redo.len(), s.revision), history);
}

#[test]
fn appearance_replace_scope_deduplicates_and_does_not_expand_component_targets() {
    let mut s = Session::default();
    s.execute("component.create", &json!({"name":"Part"})).unwrap();
    s.execute("solid.box", &json!({"body_name":"Specimen","length":10,"width":20,"height":5})).unwrap();
    let part = s.doc.find_component("Part").unwrap();
    let assign = json!({"bodies":["Specimen"],"components":["Part"],"body":"Specimen","faces":[[5,10,5]],"appearance":"ABS - Red"});
    s.execute("appearance.assign", &assign).unwrap();
    let history = s.undo.len();
    assert_eq!(
        s.execute(
            "appearance.replace",
            &json!({"from":"ABS - Red","to":{"color":"#123456","opacity":0.4},"scope":{"bodies":["Specimen","Specimen"]}})
        )
        .unwrap()["count"],
        2
    );
    assert_eq!(s.undo.len(), history + 1);
    assert_eq!(s.doc.appearances.components[&part].name, "ABS - Red");
    assert_eq!(
        s.execute("appearance.replace", &json!({"from":"ABS - Red","to":"ABS - Blue","scope":{"components":[part,"Part",part.to_string()]}}))
            .unwrap()["count"],
        1
    );
    assert_eq!(s.doc.appearances.bodies["Specimen"].color, [18, 52, 86]);
    // Empty scope is an explicit no-op, and a no-op preserves an existing redo entry.
    s.execute("edit.undo", &json!({})).unwrap();
    let doc = s.doc.clone();
    let history = (s.undo.len(), s.redo.len(), s.revision);
    assert_eq!(s.execute("appearance.replace", &json!({"from":"ABS - Red","to":"ABS - Blue","scope":{"bodies":[]}})).unwrap()["count"], 0);
    assert!(Arc::ptr_eq(&doc, &s.doc));
    assert_eq!((s.undo.len(), s.redo.len(), s.revision), history);
}

#[test]
fn appearance_chain_and_replace_hostile_inputs_refuse_atomically() {
    let mut s = fixture();
    s.execute("appearance.assign", &json!({"bodies":["Specimen"],"appearance":"ABS - Red"})).unwrap();
    let doc = s.doc.clone();
    let history = (s.undo.len(), s.redo.len(), s.revision);
    for target in [
        json!({}),
        json!({"body":"missing"}),
        json!({"component":"missing"}),
        json!({"component":0}),
        json!({"body":"Specimen","face":u64::MAX}),
        json!({"body":"Specimen","face":-1}),
        json!({"body":"Specimen","component":1}),
        json!({"body":["Specimen"]}),
        json!({"body":"Specimen","face":null}),
    ] {
        assert!(s.execute("appearance.chain", &json!({"target":target})).is_err());
    }
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        // JSON has no nonfinite numbers: serde_json encodes these as null.
        for look in [
            json!({"color":[value,0,0]}),
            json!({"color":"#ffffff","opacity":value}),
            json!({"color":"#ffffff","pbr":{"metallic":value,"roughness":0.3,"dielectric_ior":1.5}}),
        ] {
            for field in ["from", "to"] {
                let mut params = json!({"from":"ABS - Red","to":"ABS - Blue"});
                params[field] = look.clone();
                assert!(s.execute("appearance.replace", &params).is_err());
            }
        }
    }
    for params in [
        json!({"from":"ABS - Red","to":null}),
        json!({"from":"unknown","to":"ABS - Blue"}),
        json!({"from":"ABS - Red","to":{"color":[256,0,0]}}),
        json!({"from":"ABS - Red","to":{"appearance":"ABS - Blue","opacity":2}}),
        json!({"from":"ABS - Red","to":{"appearance":"starter.glass.clear-soda-lime","color":"#ffffff","pbr":null}}),
        json!({"from":"ABS - Red","to":"ABS - Blue","scope":{"bodies":["Specimen","missing"]}}),
        json!({"from":"ABS - Red","to":"ABS - Blue","scope":{"components":["missing"]}}),
        json!({"from":"ABS - Red","to":"ABS - Blue","scope":{"bodies":["Specimen"],"components":[false]}}),
        json!({"from":"ABS - Red","to":"ABS - Blue","scope":{"bodies":vec!["Specimen";10_001]}}),
        json!({"from":"ABS - Red","to":"ABS - Blue","scope":{"bodies":vec!["Specimen";5001],"components":vec!["missing";5000]}}),
        json!({"from":"ABS - Red","to":"ABS - Blue","scope":{"bodies":"Specimen"}}),
        json!({"from":"ABS - Red","to":"ABS - Blue","scope":{"faces":[]}}),
    ] {
        assert!(s.execute("appearance.replace", &params).is_err(), "{params}");
        assert!(Arc::ptr_eq(&doc, &s.doc));
        assert_eq!((s.undo.len(), s.redo.len(), s.revision), history);
    }
}

#[test]
fn appearance_chain_and_replace_process_ten_thousand_entries_with_bounded_output() {
    let mut s = fixture();
    let look = crate::doc::appearance::library("ABS - Red").unwrap();
    for i in 0..10_000 {
        // Suppressed definition overrides are still uses and must be replaced.
        s.doc_mut().appearances.bodies.insert(format!("Dormant {i}"), look.clone());
    }
    let chain = chain_read(&mut s, json!({"body":"Specimen"}));
    assert_eq!(chain["steps"].as_array().unwrap().len(), 4);
    assert!(chain["active"].is_null());
    assert_eq!(s.execute("appearance.replace", &json!({"from":"ABS - Red","to":"ABS - Blue"})).unwrap(), json!({"count":10_000}));
    assert!(s.doc.appearances.bodies.values().all(|l| l.name == "ABS - Blue"));
    assert_eq!(
        s.execute("appearance.replace", &json!({"from":"ABS - Blue","to":"ABS - Red","scope":{"bodies":vec!["Specimen";10_000]}})).unwrap()["count"],
        0
    );
}

#[test]
fn appearance_gestures_copy_paste_preserve_full_resolved_look_and_history() {
    let mut s = fixture();
    s.execute(
        "appearance.assign",
        &json!({"body":"Specimen","faces":[[5,10,5]],"name":"Sampled","color":"#345678","pbr":{"metallic":0.2,"roughness":0.3,"dielectric_ior":1.6}}),
    )
    .unwrap();
    let face = s.execute("model.faces", &json!({"body":"Specimen"})).unwrap()["faces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["point"][2].as_f64() == Some(5.0))
        .unwrap()["index"]
        .clone();
    let before = s.doc.clone();
    let undo = s.undo.len();
    let target = json!({"body":"Specimen","face":face});
    let sampled = s.execute("appearance.pick_up", &json!({"target":target})).unwrap();
    assert_eq!(sampled["look"]["name"], "Sampled");
    assert_eq!(s.execute("appearance.copy", &json!({"target":target})).unwrap(), sampled);
    s.execute("appearance.paint", &sampled["look"]).unwrap();
    assert!(Arc::ptr_eq(&before, &s.doc));
    assert_eq!(s.undo.len(), undo);
    s.execute("appearance.paste", &json!({"bodies":["Specimen"]})).unwrap();
    assert_eq!(s.undo.len(), undo + 1);
    assert_eq!(serde_json::to_value(&s.doc.appearances.bodies["Specimen"]).unwrap()["pbr"], sampled["look"]["pbr"]);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc.to_json(), before.to_json());
    let document = s.doc.clone();
    let history = (s.undo.len(), s.redo.len());
    for (id, params) in [
        ("appearance.pick_up", json!({"target":{"body":"missing"}})),
        ("appearance.copy", json!({"target":{"body":"Specimen","face":999999}})),
        ("appearance.paste", json!({"bodies":["Specimen","missing"]})),
        ("appearance.paint", json!({"appearance":"starter.glass.clear-soda-lime"})),
    ] {
        assert!(s.execute(id, &params).is_err(), "{id}");
        assert!(Arc::ptr_eq(&document, &s.doc));
        assert_eq!((s.undo.len(), s.redo.len()), history);
    }
}

#[test]
fn appearance_gestures_favorite_is_a_validated_transient_request() {
    let mut s = fixture();
    let before = s.doc.clone();
    let state = (s.undo.len(), s.redo.len(), s.revision, s.evaluations, s.is_dirty());
    for favorite in [true, false] {
        let reply = s.execute("appearance.favorite", &json!({"look":{"appearance":"Copper"},"favorite":favorite})).unwrap();
        assert_eq!(reply["look"]["name"], "Copper");
        assert_eq!(reply["favorite"], favorite);
    }
    for params in [
        json!(null),
        json!([]),
        json!({}),
        json!({"look":{},"favorite":true}),
        json!({"look":{"color":"#123456"},"favorite":"true"}),
        json!({"look":{"color":"#123456"},"favorite":true,"extra":1}),
    ] {
        assert!(s.execute("appearance.favorite", &params).is_err());
    }
    assert!(Arc::ptr_eq(&before, &s.doc));
    assert_eq!((s.undo.len(), s.redo.len(), s.revision, s.evaluations, s.is_dirty()), state);
}
