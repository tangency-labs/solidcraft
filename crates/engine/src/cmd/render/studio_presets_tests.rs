use super::*;
fn subject() -> Session {
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length":20,"width":15,"height":10,"body_name":"Subject"})).unwrap();
    s.execute(
        "render.settings.set",
        &json!({"name":"Shot","set":{"width":96,"height":64,
        "job":{"method":"gpu_path","options":{"samples":37,"max_depth":7,"seed":91}}}}),
    )
    .unwrap();
    s
}
#[cfg(feature = "bundled-render-library")]
#[test]
fn native_studios_resolve_without_mutation_and_apply_one_complete_undo() {
    let entries = super::entries();
    let entries = entries.as_array().unwrap();
    assert!(!entries.is_empty());
    let mut ids = std::collections::BTreeSet::new();
    let hdrs: std::collections::BTreeSet<_> = entries.iter().map(|item| item["hdr"].as_str().unwrap()).collect();
    assert!(hdrs.len() >= 24, "every authored HDR has a complete curated composition");
    for entry in entries {
        let id = entry["id"].as_str().unwrap();
        assert!(!id.is_empty() && ids.insert(id));
        let mut s = subject();
        let before = s.doc.to_json();
        let undo = s.undo.len();
        let old = s.doc.render.setups["Shot"].clone();
        let revision = s.revision;
        let plan = s.execute("render.preset.resolve", &json!({"id":id,"name":"Shot","expected_revision":revision})).unwrap();
        assert_eq!(plan["document_changed"], false);
        assert_eq!(s.doc.to_json(), before);
        assert_eq!(s.undo.len(), undo);
        let request = json!({"id":id,"name":"Shot","expected_revision":revision,"replace_lighting_and_environment":true});
        let applied = s.execute("render.preset.apply", &request).unwrap();
        assert_eq!(applied["document_changed"], true);
        assert_eq!(s.undo.len(), undo + 1);
        let saved = &s.doc.render.setups["Shot"];
        assert_eq!((saved.width, saved.height, saved.camera), (old.width, old.height, old.camera));
        let Some(RenderJob::GpuPath { options, .. }) = &saved.job else { panic!("explicit GPU intent lost") };
        assert_eq!((options.samples, options.max_depth, options.seed), (37, 7, 91));
        assert!(options.hdr.is_some());
        assert!(!saved.stage.is_empty());
        assert_eq!(s.doc.features.len(), 1);
        assert!(s.doc.materials.is_empty());
        let reopened = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
        assert_eq!(reopened.render.setups["Shot"], *saved);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc.to_json(), before);
    }
}
#[test]
fn native_studio_refusals_are_atomic_and_never_silently_replace_resources() {
    let mut s = subject();
    let revision = s.revision;
    let before = s.doc.to_json();
    for request in [
        json!({"id":"SCENE-001","name":"Shot","expected_revision":revision,"replace_lighting_and_environment":true}),
        json!({"id":"NATIVE-STUDIO-001","name":"Shot","expected_revision":revision}),
        json!({"id":"NATIVE-STUDIO-001","name":"Shot","replace_lighting_and_environment":true}),
        json!({"id":"NATIVE-STUDIO-001","name":"Shot","expected_revision":revision+1,"replace_lighting_and_environment":true}),
        json!({"id":"NATIVE-STUDIO-001","name":"Shot","expected_revision":revision,"replace_lighting_and_environment":true,"set":{"job":{"method":"raster"}}}),
    ] {
        assert!(s.execute("render.preset.apply", &request).is_err());
        assert_eq!(s.doc.to_json(), before);
    }
    s.execute("render.stage.ground", &json!({"name":"Shot"})).unwrap();
    let before = s.doc.to_json();
    let revision = s.revision;
    assert!(
        s.execute(
            "render.preset.apply",
            &json!({"id":"NATIVE-STUDIO-001","name":"Shot","expected_revision":revision,
        "set":{"stage":[]},"replace_lighting_and_environment":true})
        )
        .is_err()
    );
    assert_eq!(s.doc.to_json(), before);
    let mut empty = Session::default();
    assert!(
        empty
            .execute("render.preset.resolve", &json!({"id":"NATIVE-STUDIO-001","name":"E","set":{"job":{"method":"gpu_path","options":{}}}}))
            .is_err()
    );
}
#[test]
fn saved_shot_commands_preserve_exact_resources_reject_collisions_and_undo() {
    let mut s = subject();
    let before = s.doc.to_json();
    let revision = s.revision;
    s.execute("render.shot.duplicate", &json!({"name":"Shot","new_name":"Copy","expected_revision":revision})).unwrap();
    assert_eq!(s.doc.render.setups["Shot"], s.doc.render.setups["Copy"]);
    let duplicated = s.doc.to_json();
    for (command, p) in [
        ("render.shot.duplicate", json!({"name":"Shot","new_name":"Copy","expected_revision":s.revision})),
        ("render.shot.rename", json!({"name":"Shot","new_name":"Copy","expected_revision":s.revision})),
        ("render.shot.remove", json!({"name":"Shot","expected_revision":revision})),
        ("render.shot.remove", json!({"name":"Shot","new_name":"other","expected_revision":s.revision})),
        ("render.shot.duplicate", json!({"name":"Shot","new_name":"x".repeat(129),"expected_revision":s.revision})),
    ] {
        assert!(s.execute(command, &p).is_err());
        assert_eq!(s.doc.to_json(), duplicated);
    }
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc.to_json(), before);
    s.execute("render.shot.rename", &json!({"name":"Shot","new_name":"Renamed","expected_revision":s.revision})).unwrap();
    assert!(!s.doc.render.setups.contains_key("Shot"));
    assert!(s.doc.render.setups.contains_key("Renamed"));
    s.execute("render.shot.remove", &json!({"name":"Renamed","expected_revision":s.revision})).unwrap();
    assert!(s.doc.render.setups.is_empty());
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(s.doc.render.setups.contains_key("Renamed"));
}
#[test]
fn quality_projection_never_claims_adaptive_or_unbounded_profiles() {
    let mut s = subject();
    let before = s.doc.to_json();
    for id in ["Q-001", "Q-002", "Q-003"] {
        let plan = s.execute("render.quality.prepare", &json!({"id":id,"variant":"fixed_samples_png","acknowledge_omissions":true})).unwrap();
        assert_eq!(plan["document_changed"], false);
        assert_eq!(plan["status"], "fixed_budget_projection_not_full_quality_profile");
        assert!(plan["omitted"].as_array().unwrap().iter().any(|v| v == "adaptive_stopping_and_error_target"));
        assert!(plan["gpu_options"]["samples"].as_u64().unwrap() <= 4096);
        assert_eq!(s.doc.to_json(), before);
    }
    for id in ["Q-004", "Q-005", "Q-006", "Q-007", "Q-008", "Q-009", "Q-010", "missing"] {
        assert!(s.execute("render.quality.prepare", &json!({"id":id,"variant":"fixed_samples_png","acknowledge_omissions":true})).is_err());
    }
    assert!(s.execute("render.quality.prepare", &json!({"id":"Q-002","variant":"full","acknowledge_omissions":true})).is_err());
    assert!(s.execute("render.quality.prepare", &json!({"id":"Q-002","variant":"fixed_samples_png","acknowledge_omissions":false})).is_err());
    assert_eq!(s.doc.to_json(), before);
}
#[cfg(feature = "bundled-render-library")]
#[test]
fn resource_availability_and_original_previews_are_bounded_and_nonmutating() {
    let mut s = subject();
    let before = s.doc.to_json();
    assert_eq!(s.execute("render.library.list", &json!({"kind":"native_scenes","availability":"usable"})).unwrap()["total"], 25);
    assert_eq!(s.execute("render.library.list", &json!({"kind":"materials","availability":"usable"})).unwrap()["total"], 0);
    assert_eq!(s.execute("render.library.list", &json!({"kind":"materials","availability":"specification"})).unwrap()["total"], 600);
    assert_eq!(s.execute("render.library.list", &json!({"kind":"selene_shots"})).unwrap()["total"], 12);
    assert!(s.execute("render.library.list", &json!({"availability":"invented"})).is_err());
    let preview = s.execute("render.library.preview", &json!({"id":"SURF-001"})).unwrap();
    let bytes: Vec<u8> = serde_json::from_value(preview["bytes"].clone()).unwrap();
    assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
    assert_eq!(preview["sha256"], json!(solidcraft_doc::render_resources::HdrId::of(&bytes)));
    assert_eq!(preview["fidelity"], "authored_base_color_source_map_not_a_render");
    for id in ["../SURF-001", "HDR-001", "MAT-001", "SURF-999"] {
        assert!(s.execute("render.library.preview", &json!({"id":id})).is_err());
    }
    let targets = s.execute("render.library.targets", &json!({"query":"Subject"})).unwrap();
    assert_eq!(targets["items"][0]["name"], "Subject");
    assert_eq!(targets["items"][0]["opacity"], 1.0);
    assert_eq!(s.doc.to_json(), before);
}

#[test]
fn physical_backgrounds_preserve_geometry_lights_camera_and_one_undo() {
    for kind in ["floor_wall", "cyclorama"] {
        let mut s = subject();
        let before = s.doc.to_json();
        let undo = s.undo.len();
        let setup = s.doc.render.setups["Shot"].clone();
        let p = json!({"name":"Shot","kind":kind,"expected_revision":s.revision,"reflectance_linear":[0.31,0.29,0.25]});
        let result = s.execute("render.stage.backdrop", &p).unwrap();
        assert_eq!(result["engineering_geometry_unchanged"], true);
        let after = &s.doc.render.setups["Shot"];
        assert_eq!(after.camera, setup.camera);
        assert_eq!(after.job, setup.job);
        assert_eq!(after.surfaces, setup.surfaces);
        assert_eq!(after.stage.iter().count(), if kind == "cyclorama" { 26 } else { 2 });
        assert_eq!(s.doc.features.len(), 1);
        assert_eq!(s.undo.len(), undo + 1);
        let mut scene = crate::product_scene::extract(&s).unwrap();
        let first = scene.meshes[0].mesh.clone();
        super::super::staging::apply(&mut scene, after, "test").unwrap();
        assert!(Arc::ptr_eq(&first, &scene.meshes[0].mesh));
        assert_eq!(scene.meshes.len(), 1 + after.stage.iter().count());
        let reopened = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
        assert_eq!(&reopened.render.setups["Shot"], after);
        let snapshot = s.doc.to_json();
        assert!(s.execute("render.stage.backdrop", &json!({"name":"Shot","kind":kind,"expected_revision":s.revision,"set":{"stage":[]}})).is_err());
        assert_eq!(s.doc.to_json(), snapshot);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc.to_json(), before);
    }
}

#[test]
fn finishing_and_linear_master_saved_intent_survive_reopen_without_host_and_reject_conflicts() {
    let mut s = subject();
    let before = s.doc.to_json();
    let job = json!({"method":"gpu_path","options":{"samples":37,"linear_exr":true,
        "finishing":{"rgb_gain":[1.2,0.9,0.7],"saturation":0.75,"contrast":1.2,"vignette":0.2},
        "backdrop":{"mode":"radial","first_srgb":[230,220,200],"second_srgb":[25,35,45],"center":[0.4,0.3],"radius":0.8}}});
    s.execute("render.settings.set", &json!({"name":"Shot","set":{"job":job}})).unwrap();
    let after = s.doc.to_json();
    let reopened = solidcraft_doc::Document::from_json(&after).unwrap();
    assert_eq!(reopened.render, s.doc.render);
    for patch in [
        json!({"job":{"method":"gpu_path","transparent_background":true,"options":job["options"]}}),
        json!({"job":{"method":"gpu_path","options":{"finishing":{"saturation":-1}}}}),
        json!({"job":{"method":"gpu_path","options":{"backdrop":{"radius":0}}}}),
        json!({"job":{"method":"cpu_raytrace","options":{"linear_exr":true}}}),
        json!({"job":{"method":"gpu_path","options":{"adaptive_samples":true}}}),
    ] {
        assert!(s.execute("render.settings.set", &json!({"name":"Shot","set":patch})).is_err());
        assert_eq!(s.doc.to_json(), after);
    }
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc.to_json(), before);
}
