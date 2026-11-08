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

#[test]
fn cpu_quality_projects_exact_catalog_costs_with_existing_admissible_profiles_without_mutation() {
    use solidcraft_render::job::{CpuBudget, CpuProgressiveOptions};
    let mut s = subject();
    let before = s.doc.clone();
    let history = (s.revision, s.undo.len(), s.redo.len());
    for (id, width, height, samples, profile) in
        [("Q-001", 960, 540, 16, CpuBudget::Compact), ("Q-002", 1600, 900, 32, CpuBudget::Studio), ("Q-003", 2560, 1440, 64, CpuBudget::Studio)]
    {
        let plan = s.execute("render.quality.prepare", &json!({"id":id,"variant":"cpu_progressive_png","acknowledge_omissions":true})).unwrap();
        assert_eq!(plan["settings"], json!({"width":width,"height":height}));
        assert_eq!(plan["cpu_options"], json!({"budget":profile,"progressive":{"samples_per_pixel":samples,"seed":21026}}));
        assert_eq!(plan["changed_fields"], json!(["width", "height", "progressive.samples_per_pixel", "progressive.seed", "budget"]));
        assert!(plan.get("gpu_options").is_none());
        assert!(plan["cpu_options"].get("path").is_none());
        assert!(plan["cpu_options"].get("denoise").is_none());
        assert_eq!(plan["status"], "cpu_cost_budget_projection_not_full_quality_profile");
        assert_eq!(plan["document_changed"], false);
        let pixels = u64::from(width) * u64::from(height);
        assert!(pixels * u64::from(samples) <= profile.limits().max_camera_samples);
        profile.validate_image(width, height, Some(CpuProgressiveOptions { samples_per_pixel: samples, seed: 21026 })).unwrap();
        for larger in [CpuBudget::Studio, CpuBudget::Workstation] {
            let kept = s
                .execute(
                    "render.quality.prepare",
                    &json!({"id":id,"variant":"cpu_progressive_png","acknowledge_omissions":true,"cpu":{"minimum_budget":larger}}),
                )
                .unwrap();
            assert_eq!(kept["cpu_options"]["budget"], json!(larger), "an explicitly chosen larger profile must not be lowered");
        }
        assert!(Arc::ptr_eq(&before, &s.doc));
        assert_eq!(history, (s.revision, s.undo.len(), s.redo.len()));
    }
    let pilot = s
        .execute(
            "render.quality.prepare",
            &json!({"id":"Q-001","variant":"cpu_progressive_png","acknowledge_omissions":true,
        "cpu":{"adaptive":{"pilot_samples":256,"min_samples":16}}}),
        )
        .unwrap();
    assert_eq!(pilot["cpu_options"]["budget"], "studio", "Draft must budget272total passes, not hide pilots outside Compact64");
    let first_pixels = 960_u64 * 540;
    assert!(first_pixels * 272 < CpuBudget::Studio.limits().max_camera_samples);
    assert!(Arc::ptr_eq(&before, &s.doc));
    assert_eq!(history, (s.revision, s.undo.len(), s.redo.len()));
}

#[test]
fn cpu_quality_rejects_unbound_ids_hostile_context_and_incompatible_adaptive_without_silent_substitution() {
    let mut s = subject();
    let doc = s.doc.clone();
    let before = (s.revision, s.undo.len(), s.redo.len());
    for value in [
        json!({"id":"Q-004","variant":"cpu_progressive_png","acknowledge_omissions":true}),
        json!({"id":"Q-999","variant":"cpu_progressive_png","acknowledge_omissions":true}),
        json!({"id":"Q-001","variant":"cpu_progressive_png","acknowledge_omissions":false}),
        json!({"id":"Q-001","variant":"cpu_progressive_png","acknowledge_omissions":true,"cpu":{"minimum_budget":"unlimited"}}),
        json!({"id":"Q-001","variant":"cpu_progressive_png","acknowledge_omissions":true,"cpu":{"enable_gi":true}}),
        json!({"id":"Q-001","variant":"fixed_samples_png","acknowledge_omissions":true,"cpu":{}}),
        json!({"id":"Q-001","variant":"cpu_progressive_png","acknowledge_omissions":true,"cpu":{"adaptive":{"pilot_samples":16,"min_samples":32}}}),
        json!({"id":"Q-003","variant":"cpu_progressive_png","acknowledge_omissions":true,"cpu":{"adaptive":{"pilot_samples":16,"min_samples":128}}}),
    ] {
        assert!(s.execute("render.quality.prepare", &value).is_err(), "{value}");
        assert!(Arc::ptr_eq(&doc, &s.doc));
        assert_eq!(before, (s.revision, s.undo.len(), s.redo.len()));
    }
}

#[cfg(feature = "bundled-render-library")]
#[test]
fn cpu_native_studios_preserve_full_job_and_exact_hdr_rig_authority_for_all_twenty_five() {
    use solidcraft_doc::render_settings::CpuJob;
    use solidcraft_render::job::{CpuBudget, CpuPathOptions, CpuProgressiveOptions, ThinLens, adaptive::AdaptiveSampling};
    for entry in entries().as_array().unwrap() {
        let id = entry["id"].as_str().unwrap();
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length":20,"width":15,"height":10,"body_name":"Subject"})).unwrap();
        let camera = solidcraft_render::Camera { fov: 0.5, ..Default::default() };
        let options = CpuJob {
            budget: CpuBudget::Studio,
            progressive: Some(CpuProgressiveOptions { samples_per_pixel: 64, seed: 947 }),
            adaptive: Some(AdaptiveSampling::default()),
            path: Some(CpuPathOptions { max_depth: 5 }),
            denoise: true,
            lens: Some(ThinLens { aperture_radius_mm: 0.2, focus_distance_mm: 100.0 }),
            output: Some(Box::new(solidcraft_render::job::OutputOptions { linear_exr: true, ..Default::default() })),
            area_samples: Some(64),
            seed: Some(731),
            volume: Some(solidcraft_render::job::volume::HomogeneousVolume { extinction_per_mm: 0.0, ..Default::default() }),
            ..Default::default()
        };
        s.execute("render.settings.set", &json!({"name":"Shot","set":{"width":96,"height":64,"camera":camera,"job":{"method":"cpu_raytrace","transparent_background":true,"options":options}}})).unwrap();
        s.execute("render.library.environment.apply", &json!({"id":"HDR-001","name":"Shot","samples":64})).unwrap();
        let before = s.doc.clone();
        let undo = s.undo.len();
        let revision = s.revision;
        let old = s.doc.render.setups["Shot"].clone();
        let plan = s.execute("render.preset.resolve", &json!({"id":id,"name":"Shot","expected_revision":revision})).unwrap();
        assert!(Arc::ptr_eq(&s.doc, &before));
        assert_eq!(s.undo.len(), undo);
        assert_eq!(plan["executor"], "cpu-raytrace");
        let applied = s
            .execute("render.preset.apply", &json!({"id":id,"name":"Shot","expected_revision":revision,"replace_lighting_and_environment":true}))
            .unwrap();
        assert_eq!(s.undo.len(), undo + 1);
        assert_eq!(applied["setup"], plan["setup"]);
        let saved = &s.doc.render.setups["Shot"];
        assert_eq!((saved.camera, saved.width, saved.height), (old.camera, old.width, old.height));
        assert_eq!((&saved.optics, &saved.coats, &saved.surfaces), (&old.optics, &old.coats, &old.surfaces));
        let Some(RenderJob::CpuRaytrace { options, transparent_background }) = &saved.job else { panic!("CPU method lost") };
        assert!(*transparent_background);
        let Some(RenderJob::CpuRaytrace { options: old_options, .. }) = &old.job else {
            panic!("the captured original setup must retain its CPU method")
        };
        let mut expected = (**old_options).clone();
        expected.preset = None;
        expected.lights = Some(vec![]);
        expected.punctual_lights = Some(vec![]);
        expected.environment = None;
        expected.sky = None;
        expected.sun = None;
        expected.hdr = Some(CpuHdrEnvironment {
            resource: serde_json::from_value(plan["resource_sha256"].clone()).unwrap(),
            rotation_rad: 0.35 + std::f64::consts::PI,
            intensity: entry["intensity"].as_f64().unwrap(),
            samples: 64,
        });
        expected.area_lights = Some(vec![]);
        if entry["rig"] == "RIG-002" {
            let bounds = crate::product_scene::extract(&s).unwrap().bounds;
            let size = bounds.max - bounds.min;
            let c = bounds.center();
            let (lighting, _) = super::super::library::rig_lighting(
                &super::super::library::RigInput {
                    id: "RIG-002".into(),
                    variant: "complete".into(),
                    center_mm: [c.x, c.y, c.z],
                    subject_extent_mm: size.x.max(size.y).max(size.z),
                },
                "test",
            )
            .unwrap();
            expected.area_lights = Some(
                lighting
                    .area_lights
                    .iter()
                    .map(|light| solidcraft_render::job::RectangleLightOptions {
                        center_mm: light.center,
                        edge_u_mm: light.u.map(|x| x * 2.0),
                        edge_v_mm: light.v.map(|x| x * 2.0),
                        color: [1.0; 3],
                        intensity: light.radiance[0] / 16.0,
                    })
                    .collect(),
            );
            assert_eq!(expected.area_lights.as_ref().unwrap().iter().map(|a| a.intensity).fold(0.0, f64::max), 29.691586369625);
        }
        assert_eq!(**options, expected, "all unrelated CPU intent survives {id}");
        let hdr = options.hdr.unwrap();
        assert_eq!(
            s.doc.render.resources.required(&hdr.resource).unwrap().bytes(),
            super::super::library_data::asset(entry["hdr"].as_str().unwrap()).unwrap().0
        );
        assert_eq!(s.doc.features, before.features);
        assert_eq!(s.doc.materials, before.materials);
        let saved_json = s.doc.to_json();
        let reopened = solidcraft_doc::Document::from_json(&saved_json).unwrap();
        assert_eq!(reopened.render.setups["Shot"], *saved);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc, before);
        s.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(s.doc.to_json(), saved_json);
    }
}

#[cfg(all(feature = "bundled-render-library", feature = "raytrace-cpu", not(target_arch = "wasm32")))]
#[test]
fn cpu_native_studio_cold_capture_matches_typed_source_every_prefix_and_raw_master() {
    use solidcraft_render::job::{CancelToken, CpuHost, ImageRequest, Integrator, JobId, Phase};
    use solidcraft_render::lighting::environment_input::EnvironmentInput;
    use solidcraft_render::raytrace::image::{ImagePoll, ImageTask};
    for id in ["NATIVE-STUDIO-001", "NATIVE-STUDIO-004", "RIG-020"] {
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length":20,"width":15,"height":10,"body_name":"Subject"})).unwrap();
        let camera = solidcraft_render::Camera { target: solidcore_record::Vec3::new(10.0, 7.5, 5.0), distance: 35.0, ..Default::default() };
        s.execute("render.settings.set",&json!({"name":"Shot","set":{"width":8,"height":8,"camera":camera,"job":{"method":"cpu_raytrace","options":{"progressive":{"samples_per_pixel":3,"seed":47},"area_samples":1,"seed":731,"output":{"linear_exr":true}}}}})).unwrap();
        s.execute("render.library.environment.apply", &json!({"id":"HDR-001","name":"Shot","samples":1})).unwrap();
        if id == "RIG-020" {
            s.execute("render.library.studio.apply", &json!({"name":"Shot","rig":{"id":id,"variant":"complete","center_mm":[10,7.5,5],"subject_extent_mm":20},"expected_revision":s.revision,"replace_lighting_and_environment":true,"enable_indirect":true})).unwrap();
        } else {
            s.execute("render.preset.apply", &json!({"id":id,"name":"Shot","expected_revision":s.revision,"replace_lighting_and_environment":true}))
                .unwrap();
        }
        let bytes = solidcraft_io::write_design_container(&s.doc).unwrap();
        s = Session::new(solidcraft_io::read_design(&bytes).unwrap());
        s.execute("render.scene", &json!({})).unwrap();
        let setup = s.doc.render.setups["Shot"].clone();
        let Some(RenderJob::CpuRaytrace { options, transparent_background }) = &setup.job else { panic!() };
        let hdr = options.hdr;
        if id == "RIG-020" {
            assert!(hdr.is_none());
            assert!(options.sun.is_some());
        } else {
            assert_eq!(hdr.unwrap().samples, 1);
        }
        let runtime = options.to_runtime(setup.exposure_ev).unwrap();
        let mut scene = crate::product_scene::extract(&s).unwrap();
        super::super::staging::apply(&mut scene, &setup, "test").unwrap();
        let render = |quantum| {
            let request = ImageRequest {
                integrator: Integrator::Raytrace,
                raytrace: Some(runtime.clone()),
                camera: setup.camera,
                width: setup.width,
                height: setup.height,
                background: setup.background,
                transparent_background: *transparent_background,
                studio_weights: None,
                environment: None,
                textures: None,
            };
            let input = hdr.map(|hdr| {
                EnvironmentInput::new(s.doc.render.resources.required(&hdr.resource).unwrap().source(), hdr.rotation_rad, hdr.intensity, hdr.samples)
                    .unwrap()
            });
            let mut task = ImageTask::new_with_environment(scene.clone(), request, CancelToken::default(), runtime.clone(), input).unwrap();
            let mut prefixes = Vec::new();
            for _ in 0..2_000_000 {
                match task.step_with_previews(quantum).unwrap() {
                    ImagePoll::Pending(_) => {}
                    ImagePoll::Preview { png, samples_per_pixel } => prefixes.push((samples_per_pixel, png)),
                    ImagePoll::Complete(png) => return (prefixes, png, task.take_linear_exr().unwrap()),
                }
            }
            panic!("bounded native studio source failed to finish")
        };
        let expected = render(1);
        assert_eq!(expected.0.iter().map(|(sample, _)| *sample).collect::<Vec<_>>(), [1, 2]);
        assert!(expected.0.iter().all(|(_, png)| png.starts_with(b"\x89PNG\r\n\x1a\n")));
        assert_eq!(expected, render(4096), "every complete prefix, final PNG and raw master is independent of work quantum");
        let host = CpuHost::for_platform().unwrap();
        s.set_render_job_host(Some(host)).unwrap();
        let doc = s.doc.clone();
        let history = (s.revision, s.undo.len(), s.redo.len());
        s.execute("render.preflight", &json!({"settings":setup})).unwrap();
        let reply = s.execute("render.start", &json!({"settings":setup})).unwrap();
        let job: JobId = serde_json::from_value(reply["job"].clone()).unwrap();
        let handle = s.render_job_handle(job).unwrap();
        let status = handle.wait_timeout(std::time::Duration::from_secs(60)).unwrap();
        assert_eq!(status.progress.phase, Phase::Completed, "{:?}", status.error);
        let frame = handle.frame().unwrap().unwrap();
        assert_eq!(frame.png(), expected.1.as_slice());
        assert_eq!(frame.linear_exr().unwrap(), expected.2.as_slice());
        assert!(Arc::ptr_eq(&doc, &s.doc));
        assert_eq!((s.revision, s.undo.len(), s.redo.len()), history);
    }
}
