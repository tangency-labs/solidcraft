use super::*;

fn save(s: &mut Session, patch: Value) {
    s.execute("render.settings.set", &json!({"name":"Studio", "set":patch})).unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn embedded_hdr_capture_is_offline_atomic_deduplicated_and_undoable_after_source_removal() {
    use solidcraft_doc::render_resources::HdrId;
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let path = std::env::temp_dir().join(format!("solidcraft-saved-hdr-{}-{nonce}.hdr", std::process::id()));
    let mut bytes = b"#?RADIANCE\nFORMAT=32-bit_rle_rgbe\nPRIMARIES=0.64 0.33 0.30 0.60 0.15 0.06 0.3127 0.3290\n\n-Y 2 +X 4\n".to_vec();
    for _ in 0..8 {
        bytes.extend_from_slice(&[128, 64, 32, 129]);
    }
    std::fs::write(&path, &bytes).unwrap();
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length":10,"width":20,"height":5})).unwrap();
    s.mark_saved();
    s.evaluations = 0;
    let original = s.doc.clone();
    let model = s.model.state();
    let before = (s.revision, s.undo.len(), s.redo.len());
    let info = s.execute("render.hdr.inspect", &json!({"path":path})).unwrap();
    assert_eq!(info["sha256"], json!(HdrId::of(&bytes)));
    assert!(info.get("data").is_none());
    assert!(Arc::ptr_eq(&original, &s.doc));
    assert_eq!(before, (s.revision, s.undo.len(), s.redo.len()));
    let params = json!({"name":"HDR","path":path,"expected_sha256":info["sha256"],"rotation_rad":0.75,"intensity":2.0,
        "set":{"width":16,"height":12,"exposure_ev":1.5,"job":{"method":"gpu_path","options":{
            "samples":37,"max_depth":9,"seed":u32::MAX,"denoise":true,"tone_map":"aces_fitted",
            "lighting":{"environment":[0.2,0.4,0.7],"area_lights":[]}}}}});
    let saved = s.execute("render.settings.capture_hdr", &params).unwrap();
    let id: HdrId = serde_json::from_value(saved["resource"]["sha256"].clone()).unwrap();
    assert!(s.render_job_host().is_none());
    assert_eq!(s.undo.len(), before.1 + 1);
    assert_eq!(s.doc.render.resources.len(), 1);
    assert_eq!(saved["setup"]["job"]["options"]["hdr"]["resource"], info["sha256"]);
    assert_eq!(saved["setup"]["job"]["options"]["lighting"]["environment"], json!([0.2, 0.4, 0.7]));
    let captured = s.doc.clone();
    let state = (s.revision, s.undo.len(), s.redo.len());
    assert_eq!(saved, s.execute("render.settings.capture_hdr", &params).unwrap());
    assert!(Arc::ptr_eq(&captured, &s.doc));
    assert_eq!(state, (s.revision, s.undo.len(), s.redo.len()));
    let mut changed = bytes.clone();
    *changed.last_mut().unwrap() = 130;
    std::fs::write(&path, changed).unwrap();
    assert!(s.execute("render.settings.capture_hdr", &params).unwrap_err().to_string().contains("changed since selection"));
    assert!(Arc::ptr_eq(&captured, &s.doc));
    assert_eq!(state, (s.revision, s.undo.len(), s.redo.len()));
    std::fs::remove_file(&path).unwrap();
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc, original);
    assert_eq!(s.redo.len(), 1);
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(s.doc, captured);
    assert!(std::ptr::eq(
        captured.render.resources.required(&id).unwrap().bytes().as_ptr(),
        s.doc.render.resources.required(&id).unwrap().bytes().as_ptr()
    ));
    s.execute("render.settings.set", &json!({"name":"HDR","set":{"width":20}})).unwrap();
    assert_eq!(s.doc.render.setups["HDR"].job, captured.render.setups["HDR"].job);
    assert!(std::ptr::eq(
        captured.render.resources.required(&id).unwrap().bytes().as_ptr(),
        s.doc.render.resources.required(&id).unwrap().bytes().as_ptr()
    ));
    assert!(!s.doc.to_json().contains(path.to_str().unwrap()));
    assert_eq!(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap(), *s.doc);
    let container = solidcraft_io::write_design_container(&s.doc).unwrap();
    assert_eq!(solidcraft_io::read_design(&container).unwrap(), *s.doc);
    assert!(Arc::ptr_eq(&model, &s.model.state()));
    assert_eq!(s.evaluations, 0);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn embedded_hdr_capture_rejects_parameters_scope_and_cpu_primaries_without_mutation() {
    let mut s = Session::default();
    let before = s.doc.clone();
    let state = (s.revision, s.undo.len(), s.redo.len());
    for (patch, extra, expected) in [
        (json!({"job":{"method":"raster"}}), json!({}), "typed cpu_raytrace or gpu_path"),
        (json!({"job":{"method":"cpu_raytrace","options":{"primary_samples":3}}}), json!({}), "primary"),
        (json!({"job":{"method":"cpu_raytrace"}}), json!({"intensity":65}), "intensity"),
        (json!({"job":{"method":"cpu_raytrace"}}), json!({"samples":3}), "samples"),
        (json!({"job":{"method":"cpu_raytrace"}}), json!({"assume_linear_srgb":true}), "GPU-only"),
        (json!({"job":{"method":"cpu_raytrace"}}), json!({"assume_linear_srgb":false}), "GPU-only"),
        (json!({"job":{"method":"cpu_raytrace"}}), json!({"assume_linear_srgb":null}), "GPU-only"),
        (json!({"job":{"method":"gpu_path"}}), json!({"samples":16}), "CPU-only"),
        (json!({"job":{"method":"gpu_path"}}), json!({"samples":null}), "CPU-only"),
        (json!({"job":{"method":"gpu_path"}}), json!({"intensity":101}), "intensity"),
        (json!({"job":{"method":"gpu_path","options":{"environment":{"kind":"sky"}}}}), json!({}), "unset"),
    ] {
        let mut params = json!({"name":"HDR","path":"must-not-be-opened.hdr","set":patch});
        params.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        let error = s.execute("render.settings.capture_hdr", &params).unwrap_err().to_string();
        assert!(error.contains(expected), "{error}");
        assert!(Arc::ptr_eq(&before, &s.doc));
        assert_eq!(state, (s.revision, s.undo.len(), s.redo.len()));
    }
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("solidcraft-hdr-scope-{}-{nonce}", std::process::id()));
    std::fs::create_dir(&dir).unwrap();
    let allowed = dir.join("allowed");
    std::fs::create_dir(&allowed).unwrap();
    let path = dir.join("source.hdr");
    let mut bytes = b"#?RADIANCE\nFORMAT=32-bit_rle_rgbe\n\n-Y 2 +X 4\n".to_vec();
    for _ in 0..8 {
        bytes.extend_from_slice(&[128, 64, 32, 129]);
    }
    std::fs::write(&path, &bytes).unwrap();
    let mut capture = json!({"name":"HDR","path":path,"set":{"job":{"method":"cpu_raytrace"}}});
    solidcraft_io::vfs::scoped(Some(solidcraft_io::vfs::PathScope::new([allowed])), || {
        for (command, params) in [("render.hdr.inspect", json!({"path":path})), ("render.settings.capture_hdr", capture.clone())] {
            assert!(s.execute(command, &params).unwrap_err().to_string().contains("outside the folders this agent may use"));
        }
    });
    assert!(s.execute("render.settings.capture_hdr", &capture).unwrap_err().to_string().contains("primaries"));
    capture["set"]["job"]["method"] = json!("gpu_path");
    assert!(s.execute("render.settings.capture_hdr", &capture).unwrap_err().to_string().contains("primaries"));
    assert!(Arc::ptr_eq(&before, &s.doc));
    assert_eq!(state, (s.revision, s.undo.len(), s.redo.len()));
    let info = s.execute("render.hdr.inspect", &json!({"path":path})).unwrap();
    capture["expected_sha256"] = info["sha256"].clone();
    capture["assume_linear_srgb"] = json!(true);
    let reply = s.execute("render.settings.capture_hdr", &capture).unwrap();
    assert_eq!(reply["setup"]["job"]["options"]["hdr"]["assume_linear_srgb"], true);
    let saved = s.doc.clone();
    std::fs::write(&path, b"not an HDR source").unwrap();
    assert!(s.execute("render.hdr.inspect", &json!({"path":path})).is_err());
    assert!(Arc::ptr_eq(&saved, &s.doc));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn portable_job_save_is_host_independent_atomic_and_undoable_without_geometry_changes() {
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length":10,"width":20,"height":5})).unwrap();
    s.mark_saved();
    assert!(s.render_job_host().is_none());
    let original = s.doc.clone();
    let model = s.model.state();
    let world = s.world_state();
    let undo = s.undo.len();
    s.evaluations = 0;
    let setup: RenderSetup = serde_json::from_value(json!({
        "width":640,"height":480,"exposure_ev":2.5,"backend":"future-backend",
        "job":{"method":"gpu_path","options":{"samples":256,"max_depth":12,"seed":73,"tone_map":"aces_fitted","denoise":true,
            "lighting":{"environment":[0.2,0.4,0.8],"area_lights":[]},
            "environment":{"kind":"sky","rotation_rad":0.5,"intensity":2}}}
    }))
    .unwrap();
    // A complete setup has eleven fields, including the optional job.
    let full = serde_json::to_value(&setup).unwrap();
    assert_eq!(full.as_object().unwrap().len(), 11);
    save(&mut s, full.clone());
    assert!(s.is_dirty());
    assert_eq!(s.undo.len(), undo + 1);
    assert_eq!(s.doc.render.setups["Studio"], setup);
    assert!(s.doc.differs_only_in_render(&original));
    let saved = s.doc.clone();
    let revision = s.revision;
    save(&mut s, full);
    assert!(Arc::ptr_eq(&saved, &s.doc));
    assert_eq!(s.revision, revision);
    assert_eq!(s.execute("render.settings.get", &json!({"name":"Studio"})).unwrap()["setup"], json!(setup));
    let reopened = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
    assert_eq!(*s.doc, reopened);
    save(&mut s, json!({"width":800}));
    assert_eq!(s.doc.render.setups["Studio"].job, setup.job);
    s.execute("edit.undo", &json!({})).unwrap();
    let revision = s.revision;
    save(&mut s, json!({"job":setup.job}));
    assert_eq!(s.revision, revision);
    assert_eq!(s.redo.len(), 1, "an identical job preserves redo");
    s.execute("edit.redo", &json!({})).unwrap();
    save(&mut s, json!({"job":null}));
    assert!(s.doc.render.setups["Studio"].job.is_none());
    assert_eq!(s.doc.render.setups["Studio"].exposure_ev, 2.5);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc.render.setups["Studio"].job, setup.job);
    save(&mut s, json!({"job":{"method":"cpu_raytrace"}}));
    assert_eq!(
        s.doc.render.setups["Studio"].job,
        Some(solidcraft_doc::render_settings::RenderJob::CpuRaytrace { options: solidcraft_doc::render_settings::CpuJob::default() })
    );
    assert_eq!(s.doc.render.setups["Studio"].exposure_ev, 2.5);
    assert!(Arc::ptr_eq(&model, &s.model.state()));
    assert!(Arc::ptr_eq(&world, &s.world_state()));
    assert_eq!(s.evaluations, 0);
}

#[test]
fn invalid_saved_jobs_never_partially_replace_a_setup() {
    let mut s = Session::default();
    save(&mut s, json!({"job":{"method":"cpu_raytrace","options":{"primary_samples":4}},"exposure_ev":2}));
    for patch in [
        json!({"width":800,"job":{"method":"cpu_raytrace","options":{"primary_samples":3}}}),
        json!({"job":{"method":"gpu_path","options":{"samples":4097}}}),
        json!({"job":{"method":"cpu_raytrace","options":{"emissions":[]}}}),
        json!({"job":{"method":"gpu_path","options":{"environment":{"kind":"hdr","path":"not-opened.hdr"}}}}),
        json!({"job":{"method":"raster"}}),
        json!({"job":{"method":"gpu_path"},"environment_intensity":2}),
        json!({"job":{"options":{"samples":4}}}),
    ] {
        let doc = s.doc.clone();
        let state = (s.revision, s.undo.len(), s.redo.len(), s.evaluations);
        assert!(s.execute("render.settings.set", &json!({"name":"Studio","set":patch})).is_err());
        assert!(Arc::ptr_eq(&doc, &s.doc));
        assert_eq!(state, (s.revision, s.undo.len(), s.redo.len(), s.evaluations));
    }
}

#[test]
fn settings_history_preserves_geometry_cache_and_dirty_state() {
    let mut s = Session::default();
    let component = s.execute("component.create", &json!({"name":"Part","grounded":false})).unwrap();
    s.execute("solid.box", &json!({"length":10,"width":20,"height":5})).unwrap();
    s.execute("component.activate", &json!({"component":0})).unwrap();
    s.execute("occurrence.move", &json!({"occurrence":component["occurrence"],"translate":[30,0,0]})).unwrap();
    s.mark_saved();
    let model = s.model.state();
    let world = s.world_state().unwrap();
    assert!(!Arc::ptr_eq(&model, &world), "exercise the prepared occurrence cache");
    let mesh = model.bodies[0].mesh();
    let undo = s.undo.len();
    s.evaluations = 0;
    save(&mut s, json!({"exposure_ev":2,"backend":"future-backend"}));
    assert!(s.is_dirty());
    assert_eq!(s.undo.len(), undo + 1);
    let doc = s.doc.clone();
    let revision = s.revision;
    save(&mut s, json!({"exposure_ev":2}));
    assert!(Arc::ptr_eq(&doc, &s.doc));
    assert_eq!(s.revision, revision);
    assert_eq!(s.undo.len(), undo + 1);
    let restored = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
    assert_eq!(restored.render, s.doc.render);
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(!s.is_dirty());
    s.execute("render.settings.get", &json!({})).unwrap();
    assert_eq!(s.redo.len(), 1);
    s.execute("edit.redo", &json!({})).unwrap();
    let checkpoint = s.checkpoint();
    save(&mut s, json!({"width":800}));
    s.execute("edit.undo", &json!({})).unwrap();
    let revision = s.revision;
    save(&mut s, json!({"exposure_ev":2}));
    assert_eq!(s.redo.len(), 1, "a no-op edit preserves redo");
    assert_eq!(s.revision, revision);
    s.execute("edit.redo", &json!({})).unwrap();
    s.restore_checkpoint(checkpoint).unwrap();
    assert_eq!(s.doc.render.setups["Studio"].width, 1024);
    assert!(Arc::ptr_eq(&model, &s.model.state()));
    assert!(Arc::ptr_eq(&mesh, &s.model.state().bodies[0].mesh()));
    assert!(Arc::ptr_eq(&world, &s.world_state().unwrap()));
    assert_eq!(s.evaluations, 0);
    // A geometry change and its history still require normal evaluation.
    s.execute("solid.box", &json!({"length":2,"width":3,"height":4})).unwrap();
    assert!(s.evaluations > 0);
    s.evaluations = 0;
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(s.evaluations > 0);
    s.evaluations = 0;
    s.execute("edit.redo", &json!({})).unwrap();
    assert!(s.evaluations > 0);
    let checkpoint = s.checkpoint();
    let expected = s.doc.clone();
    s.execute("solid.box", &json!({"length":1,"width":1,"height":1})).unwrap();
    s.evaluations = 0;
    s.execute("edit.restore_checkpoint", &json!({"checkpoint":checkpoint})).unwrap();
    assert_eq!(*s.doc, *expected);
    assert!(s.evaluations > 0);
}

#[test]
fn invalid_settings_and_limit_are_atomic() {
    let mut s = Session::default();
    let mut camera_with_typo = serde_json::to_value(solidcraft_render::Camera::default()).unwrap();
    camera_with_typo["fov_degrees"] = json!(45);
    for patch in [
        json!({"width":0}),
        json!({"width":4097}),
        json!({"height":null}),
        json!({"unknown":true}),
        json!({"camera":{"distance":0}}),
        json!({"camera":camera_with_typo}),
        json!({"exposure_ev":33}),
        json!({"environment_intensity":101}),
    ] {
        let doc = s.doc.clone();
        let state = (s.revision, s.undo.len(), s.redo.len(), s.evaluations);
        assert!(s.execute("render.settings.set", &json!({"name":"Studio","set":patch})).is_err());
        assert!(Arc::ptr_eq(&doc, &s.doc));
        assert_eq!(state, (s.revision, s.undo.len(), s.redo.len(), s.evaluations));
    }
    for i in 0..32 {
        s.execute("render.settings.set", &json!({"name":i.to_string(),"set":{}})).unwrap();
    }
    let doc = s.doc.clone();
    assert!(s.execute("render.settings.set", &json!({"name":"overflow","set":{}})).is_err());
    assert!(Arc::ptr_eq(&doc, &s.doc));
    assert!(s.execute("render.settings.get", &json!({"name":"missing"})).is_err());
    assert_eq!(s.evaluations, 0);
}

#[test]
fn settings_history_and_checkpoint_preserve_a_stale_live_drag() {
    let mut s = Session::default();
    for (id, p) in [
        ("sketch.create", json!({"plane":"XY"})),
        ("sketch.rectangle.two_point", json!({"p0":[5,5],"p1":[25,20]})),
        ("sketch.dimension", json!({"entities":["l1"],"value":20})),
        ("sketch.dimension", json!({"entities":["l2"],"value":15})),
        ("sketch.finish", json!({})),
        ("solid.extrude", json!({"distance":5})),
        ("sketch.edit", json!({"sketch":"Sketch1"})),
        ("sketch.drag", json!({"entity":"l1.start","from":[5,5],"to":[4,5],"live":true})),
    ] {
        s.execute(id, &p).unwrap();
    }
    assert!(s.model.is_stale());
    let model = s.model.state();
    s.evaluations = 0;
    let checkpoint = s.execute("edit.checkpoint", &json!({})).unwrap()["checkpoint"].clone();
    save(&mut s, json!({"exposure_ev":1}));
    s.execute("render.settings.get", &json!({"name":"Studio"})).unwrap();
    s.execute("edit.undo", &json!({})).unwrap();
    s.execute("edit.redo", &json!({})).unwrap();
    s.execute("edit.restore_checkpoint", &json!({"checkpoint":checkpoint})).unwrap();
    assert!(s.doc.render.is_empty());
    assert!(s.model.is_stale());
    assert!(Arc::ptr_eq(&model, &s.model.state()));
    assert_eq!(s.evaluations, 0);
    s.execute("sketch.finish", &json!({})).unwrap();
    assert!(!s.model.is_stale());
    assert!(s.evaluations > 0);
}
