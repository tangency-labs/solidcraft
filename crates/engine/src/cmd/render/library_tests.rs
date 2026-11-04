use super::*;
#[cfg(feature = "bundled-render-library")]
use solidcraft_render::job::textures::{BaseColorMode, TextureMap};

#[test]
fn authored_library_search_is_bounded_and_concepts_are_not_runtime_presets() {
    let mut s = Session::default();
    for (kind, count) in [
        ("materials", 600),
        ("lighting", 280),
        ("environments", 280),
        ("surfaces", 48),
        ("selene_appearances", 48),
        ("hdr", 24),
        ("rigs", 24),
        ("scenes", 24),
        ("quality", 10),
    ] {
        let response = s.execute("render.library.list", &json!({"kind":kind,"limit":1})).unwrap();
        assert_eq!(response["total"], count);
        assert_eq!(response["items"].as_array().unwrap().len(), 1);
        assert_eq!(response["next_offset"], 1);
    }
    let response = s.execute("render.library.describe", &json!({"kind":"materials","id":"MAT-001"})).unwrap();
    assert_eq!(response["projection"]["status"], "specification_only");
    assert_eq!(response["resource"]["status"], "concept");
    for params in [
        json!({"limit":129}),
        json!({"limit":0}),
        json!({"offset":4097}),
        json!({"query":"x".repeat(257)}),
        json!({"kind":"not-a-kind"}),
        json!({"extra":true}),
    ] {
        assert!(s.execute("render.library.list", &params).is_err());
    }
    assert_eq!(s.execute("render.library.list", &json!({"kind":"surfaces","query":"SURF-001"})).unwrap()["total"], 1);
}

#[cfg(feature = "bundled-render-library")]
#[test]
fn built_in_maps_keep_exact_published_bytes_and_explicit_channel_roles() {
    let mut count = 0;
    for surface in catalog("surface_manifest", "test").unwrap() {
        let id = surface["id"].as_str().unwrap();
        for (name, role) in [
            ("base_color", MapRole::BaseColor),
            ("normal_open_gl", MapRole::NormalOpenGl),
            ("roughness", MapRole::Roughness),
            ("metallic", MapRole::Metallic),
        ] {
            let key = format!("{id}/{name}");
            let bytes = texture_bytes(&key, role, 16 * 1024 * 1024, "test").unwrap();
            assert_eq!(HdrId::of(&bytes).to_string(), surface["maps"][name]["sha256"].as_str().unwrap());
            assert_eq!(TextureMap::new(bytes, role).unwrap().dimensions(), [512, 512]);
            count += 1;
        }
    }
    assert_eq!(count, 192);
    for key in ["../SURF-001/base_color", "SURF-001/../base_color", "SURF-001/height", "SURF-999/base_color"] {
        assert!(texture_bytes(key, MapRole::BaseColor, 16 * 1024 * 1024, "test").is_err());
    }
    assert!(texture_bytes("SURF-001/normal_open_gl", MapRole::BaseColor, 16 * 1024 * 1024, "test").is_err());
    assert!(texture_bytes("SURF-001/base_color", MapRole::BaseColor, 1, "test").is_err());
}

#[cfg(feature = "bundled-render-library")]
#[test]
fn builtin_hdr_capture_is_atomic_undoable_deduplicated_and_path_independent() {
    let mut s = Session::default();
    let empty = s.doc.to_json();
    let request = json!({"id":"HDR-001","name":"Library CPU","set":{"width":64,"height":32,"job":{"method":"cpu_raytrace","options":{}}}});
    let reply = s.execute("render.library.environment.apply", &request).unwrap();
    assert_eq!(reply["source_sha256"], reply["resource"]);
    assert_eq!(reply["native_rotation_rad"], std::f64::consts::PI);
    assert_eq!(reply["self_contained"], true);
    let Some(RenderJob::CpuRaytrace { options, .. }) = &s.doc.render.setups["Library CPU"].job else {
        panic!("CPU intent");
    };
    let id = options.hdr.unwrap().resource;
    assert_eq!(s.doc.render.resources.required(&id).unwrap().bytes(), super::super::library_data::asset("HDR-001").unwrap().0);
    let saved = s.doc.to_json();
    let reopened = solidcraft_doc::Document::from_json(&saved).unwrap();
    assert_eq!(reopened.render.resources.required(&id).unwrap().id(), id);
    let before = s.undo.len();
    assert!(s.execute("render.library.environment.apply", &request).is_err(), "replacement must be explicit");
    assert_eq!(s.doc.to_json(), saved);
    assert_eq!(s.undo.len(), before);
    let mut identical = request;
    identical["replace_environment"] = json!(true);
    s.execute("render.library.environment.apply", &identical).unwrap();
    assert_eq!(s.undo.len(), before, "identical resource application is a no-op");
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc.to_json(), empty);
}

#[test]
fn authored_hdr_orientation_matches_the_documented_longitude_origin() {
    use std::f64::consts::{PI, TAU};
    for u in [0.0, 0.125, 0.25, 0.5, 0.75, 0.999] {
        for rotation in [0.0, 0.3, -1.2] {
            let world_phi = TAU * (u - 0.5) + rotation;
            let native_u = ((world_phi - hdr_rotation(rotation, "test").unwrap()) / TAU).rem_euclid(1.0);
            let distance = (native_u - u).abs();
            assert!(distance.min(1.0 - distance) < 1e-12);
        }
    }
    assert_eq!(hdr_rotation(0.0, "test").unwrap(), PI);
    for v in [f64::NAN, f64::INFINITY, 1e7] {
        assert!(hdr_rotation(v, "test").is_err());
    }
}

#[cfg(feature = "bundled-render-library")]
#[test]
fn explicit_surface_prepare_has_physical_scale_and_does_not_assign_materials() {
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length":10,"width":20,"height":5,"body_name":"Target"})).unwrap();
    let before = s.doc.to_json();
    let undo = s.undo.len();
    let reply = s.execute("render.library.surface.prepare", &json!({"id":"SURF-001","body_index":0,"body_name":"Target","scale":2.0})).unwrap();
    assert_eq!(reply["texture"]["frame"]["u"], json!([200.0, 0.0, 0.0]));
    assert_eq!(reply["texture"]["flip_v"], true);
    assert_eq!(reply["texture"]["base_color_mode"], "replace");
    assert!(reply["texture"].get("height").is_none());
    let input: super::super::textures::Override = serde_json::from_value(reply["texture"].clone()).unwrap();
    let scene = crate::product_scene::extract(&s).unwrap();
    let textures = super::super::textures::capture(&scene, &[input], "test").unwrap().unwrap();
    let binding = &textures.bindings()[0];
    assert!(binding.flip_v && binding.roughness.is_some() && binding.metallic.is_some());
    assert_eq!(binding.base_color_mode, BaseColorMode::Replace);
    assert_eq!(s.doc.to_json(), before);
    assert_eq!(s.undo.len(), undo);
    assert!(s.execute("render.library.surface.prepare", &json!({"id":"SURF-001","body_index":0,"body_name":"Old name"})).is_err());
}

#[test]
fn uniform_appearance_requires_acknowledged_projection_and_preserves_physical_data() {
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length":10,"width":20,"height":5,"body_name":"Target"})).unwrap();
    let before = s.doc.to_json();
    let before_value: Value = serde_json::from_str(&before).unwrap();
    let undo = s.undo.len();
    assert!(s.execute("render.library.appearance.apply", &json!({"id":"MAT-001","variant":"uniform_opaque","bodies":["Target"]})).is_err());
    assert_eq!(s.doc.to_json(), before);
    let reply = s.execute("render.library.appearance.apply", &json!({"id":"SURF-001","variant":"uniform_opaque","bodies":["Target"]})).unwrap();
    assert_eq!(reply["engineering_properties_unchanged"], true);
    // The middle of SURF-001's linear range, as sRGB codes.
    assert_eq!(reply["appearance"]["color"], json!([196, 202, 207]));
    let after: Value = serde_json::from_str(&s.doc.to_json()).unwrap();
    assert_eq!(after["user_materials"], before_value["user_materials"]);
    assert_eq!(s.undo.len(), undo + 1);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc.to_json(), before);
}

#[test]
fn studio_adapter_does_not_invent_passive_cards_calibration_or_extra_suns() {
    let mut s = Session::default();
    let request = json!({"id":"RIG-002","variant":"complete","center_mm":[10,20,30],"subject_extent_mm":100.0});
    let reply = s.execute("render.library.studio.prepare", &request).unwrap();
    let lighting: StudioLighting = serde_json::from_value(reply["lighting"].clone()).unwrap();
    assert_eq!(lighting.area_lights.iter().count(), 3);
    assert!(lighting.sun.is_none());
    for id in ["RIG-018", "RIG-019", "RIG-021", "RIG-023", "RIG-024"] {
        let mut p = request.clone();
        p["id"] = json!(id);
        assert!(s.execute("render.library.studio.prepare", &p).is_err());
    }
    let mut with_cards = request.clone();
    with_cards["id"] = json!("RIG-001");
    let complete = s.execute("render.library.studio.prepare", &with_cards).unwrap();
    assert_eq!(complete["stage"].as_array().unwrap().len(), 1);
    assert_eq!(complete["projection"]["passive_cards_omitted"], 0);
    let mut unknown_variant = with_cards.clone();
    unknown_variant["variant"] = json!("silently_drop_cards");
    assert!(s.execute("render.library.studio.prepare", &unknown_variant).is_err());
    with_cards["variant"] = json!("emitters_only");
    assert_eq!(s.execute("render.library.studio.prepare", &with_cards).unwrap()["projection"]["passive_cards_omitted"], 1);
    let mut sun = request;
    sun["id"] = json!("RIG-020");
    let reply = s.execute("render.library.studio.prepare", &sun).unwrap();
    let lighting: StudioLighting = serde_json::from_value(reply["lighting"].clone()).unwrap();
    let actual = lighting.sun.unwrap();
    assert!((actual.angular_radius_rad - 0.00465).abs() < 1e-12);
    assert!(actual.direction[2] > 0.0, "runtime direction points toward the sun, not along incoming photons");
}

#[cfg(feature = "bundled-render-library")]
#[test]
fn saved_authored_maps_roundtrip_and_resolve_without_paths_or_captured_ordinals() {
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length":10,"width":20,"height":5,"body_name":"Coupon"})).unwrap();
    let before = s.doc.clone();
    let undo = s.undo.len();
    let input = json!({"name":"Portable","set":{"job":{"method":"gpu_path","options":{}},"width":32,"height":32},
        "surface":{"id":"SURF-001","body_index":0,"body_name":"Coupon"}});
    s.execute("render.library.surface.bind", &input).unwrap();
    assert_eq!(s.undo.len(), undo + 1);
    assert_eq!(s.doc.materials, before.materials);
    assert_eq!(s.doc.appearances, before.appearances);
    assert_eq!(s.doc.features, before.features);
    let saved = s.doc.to_json();
    let mut reopened = Session::new(solidcraft_doc::Document::from_json(&saved).unwrap());
    reopened.execute("render.scene", &json!({})).unwrap();
    let scene = crate::product_scene::extract(&reopened).unwrap();
    let bindings = &reopened.doc.render.setups["Portable"].surfaces;
    let lowered = super::super::surface_bindings::resolve(&reopened, &scene, bindings, "test").unwrap();
    let maps = super::super::textures::capture(&scene, &lowered, "test").unwrap().unwrap();
    assert_eq!(maps.bindings().len(), 1);
    assert_eq!(maps.bindings()[0].body_name, "Coupon");
    assert_eq!(maps.bindings()[0].base_color_mode, BaseColorMode::Replace);
    assert!(maps.bindings()[0].flip_v);
    let mut changed = s.doc.render.setups["Portable"].clone();
    let mut binding = changed.surfaces.iter().next().unwrap().clone();
    binding.source.base_color_sha256 = HdrId::of(b"changed");
    changed.surfaces.set(binding, true).unwrap();
    assert!(super::super::surface_bindings::resolve(&s, &crate::product_scene::extract(&s).unwrap(), &changed.surfaces, "test").is_err());
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc, before);
}

#[cfg(feature = "bundled-render-library")]
#[test]
fn saved_surface_follows_its_occurrence_without_rebinding_another_instance() {
    let mut s = Session::default();
    let component = s.execute("component.create", &json!({"name":"Mapped part"})).unwrap();
    s.execute("solid.box", &json!({"length":10,"width":10,"height":2,"body_name":"Panel"})).unwrap();
    s.execute("component.activate", &json!({"component":0})).unwrap();
    s.execute("occurrence.copy", &json!({"component":component["component"],"translate":[30,0,0]})).unwrap();
    s.execute(
        "render.library.surface.bind",
        &json!({"name":"P","set":{"job":{"method":"gpu_path","options":{}}},
        "surface":{"id":"SURF-003","body_index":0,"body_name":"Panel"}}),
    )
    .unwrap();
    let bindings = s.doc.render.setups["P"].surfaces.clone();
    let capture = |session: &Session| {
        let scene = crate::product_scene::extract(session).unwrap();
        let lowered = super::super::surface_bindings::resolve(session, &scene, &bindings, "test").unwrap();
        super::super::textures::capture(&scene, &lowered, "test").unwrap().unwrap()
    };
    let original = capture(&s);
    let frame = original.bindings()[0].frame;
    // Upstream grounds the first occurrence. Keep that refusal explicit before
    // exercising a deliberately movable instance and its stable render binding.
    let grounded_document = s.doc.clone();
    let grounded_history = (s.revision, s.undo.len(), s.redo.len());
    let movement = json!({"occurrence":component["occurrence"],"translate":[12,0,0]});
    let error = s.execute("occurrence.move", &movement).unwrap_err().to_string();
    assert!(error.contains("is grounded"), "{error}");
    assert_eq!(s.doc, grounded_document);
    assert_eq!((s.revision, s.undo.len(), s.redo.len()), grounded_history);
    s.execute("occurrence.ground", &json!({"occurrence":component["occurrence"],"grounded":false})).unwrap();
    s.execute("occurrence.move", &movement).unwrap();
    let moved = capture(&s);
    assert_eq!(moved.bindings().len(), 1);
    assert!((moved.bindings()[0].frame.origin[0] - frame.origin[0] - 12.0).abs() < 1e-8);
    assert_eq!(moved.bindings()[0].frame.u, frame.u);
    assert_eq!(moved.bindings()[0].frame.v, frame.v);
    assert_eq!(s.doc.render.setups["P"].surfaces, bindings);
}

#[cfg(feature = "bundled-render-library")]
#[test]
fn saved_surface_rejects_changed_feature_and_hidden_target_and_can_be_removed_offline() {
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length":2,"width":2,"height":2,"body_name":"Panel"})).unwrap();
    s.execute(
        "render.library.surface.bind",
        &json!({"name":"P","set":{"job":{"method":"gpu_path","options":{}}},
        "surface":{"id":"SURF-002","body_index":0,"body_name":"Panel"}}),
    )
    .unwrap();
    let mut setup = s.doc.render.setups["P"].clone();
    let target = setup.surfaces.iter().next().unwrap().target.clone();
    let mut binding = setup.surfaces.iter().next().unwrap().clone();
    binding.target.feature += 100;
    setup.surfaces.remove(&target);
    setup.surfaces.set(binding, false).unwrap();
    let scene = crate::product_scene::extract(&s).unwrap();
    assert!(super::super::surface_bindings::resolve(&s, &scene, &setup.surfaces, "test").is_err());
    s.visibility.hidden_bodies.push("Panel".into());
    assert!(
        super::super::surface_bindings::resolve(&s, &crate::product_scene::extract(&s).unwrap(), &s.doc.render.setups["P"].surfaces, "test").is_err()
    );
    let previous = s.doc.clone();
    s.execute("render.library.surface.unbind", &json!({"name":"P","target":target})).unwrap();
    assert!(s.doc.render.setups["P"].surfaces.is_empty());
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc, previous);
    let before = s.doc.to_json();
    assert!(s.execute("render.settings.set", &json!({"name":"P","set":{"job":{"method":"raster"}}})).is_err());
    assert_eq!(s.doc.to_json(), before);
    let bindings = s.doc.render.setups["P"].surfaces.clone();
    s.execute("render.settings.set", &json!({"name":"P","set":{"job":{"method":"cpu_raytrace","options":{}}}})).unwrap();
    assert_eq!(s.doc.render.setups["P"].surfaces, bindings, "supported CPU method changes preserve saved bindings, even offline");
    assert!(s.render_job_host().is_none());
}

#[test]
fn saved_passive_ground_keeps_cad_unchanged_and_is_revalidated_after_product_motion() {
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length":10,"width":20,"height":5,"body_name":"Subject"})).unwrap();
    let before = s.doc.clone();
    let history = s.undo.len();
    let request = json!({"name":"Stage","set":{"job":{"method":"cpu_raytrace","options":{"budget":"studio"}}}});
    s.execute("render.stage.ground", &request).unwrap();
    assert_eq!(s.undo.len(), history + 1);
    assert_eq!(s.doc.features, before.features);
    assert_eq!(s.doc.materials, before.materials);
    let setup = s.doc.render.setups["Stage"].clone();
    assert_eq!(setup.stage.len(), 1);
    let original = crate::product_scene::extract(&s).unwrap();
    assert_eq!(original.meshes.len(), 1);
    let mut staged = original.clone();
    super::super::staging::apply(&mut staged, &setup, "test").unwrap();
    assert_eq!(staged.meshes.len(), 2);
    assert!(Arc::ptr_eq(&staged.meshes[0].mesh, &original.meshes[0].mesh));
    let reopened = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
    assert_eq!(reopened.render.setups["Stage"], setup);
    assert!(s.execute("render.stage.ground", &request).is_err(), "never replace a stage silently");
    s.execute("render.stage.clear", &json!({"name":"Stage"})).unwrap();
    assert!(s.doc.render.setups["Stage"].stage.is_empty());
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc.render.setups["Stage"], setup);
    let mut intersects = setup;
    let mut panel = intersects.stage.iter().next().unwrap().clone();
    panel.center_mm[2] = 0.0;
    intersects.stage = solidcraft_render::stage::Stage::new(vec![panel]).unwrap();
    assert!(super::super::staging::apply(&mut original.clone(), &intersects, "test").is_err());
}

#[test]
fn complete_authored_rig_keeps_passive_cards_nonemissive_and_scales_exactly_once() {
    let mut s = Session::default();
    let rig = json!({"id":"RIG-001","variant":"complete","center_mm":[10,20,30],"subject_extent_mm":100});
    let prepared = s.execute("render.library.studio.prepare", &rig).unwrap();
    assert_eq!(prepared["projection"]["passive_cards_omitted"], 0);
    assert_eq!(prepared["projection"]["passive_cards_included"], 1);
    assert_eq!(prepared["stage"][0]["center_mm"], json!([90.0, 10.0, 30.0]));
    let stage: solidcraft_render::stage::Stage = serde_json::from_value(prepared["stage"].clone()).unwrap();
    assert_eq!(stage.iter().next().unwrap().reflectance_linear, [0.75; 3]);
    let input = json!({"name":"Rig","set":{"job":{"method":"gpu_path","options":{}}},"rig":rig,"replace_lighting_and_environment":true});
    s.execute("render.library.studio.apply", &input).unwrap();
    assert_eq!(s.doc.render.setups["Rig"].stage, stage);
    assert!(s.doc.features.is_empty());
    assert!(s.doc.materials.is_empty());
    let mut emitter_only = rig;
    emitter_only["variant"] = json!("emitters_only");
    let projected = s.execute("render.library.studio.prepare", &emitter_only).unwrap();
    assert_eq!(projected["projection"]["passive_cards_omitted"], 1);
    assert_eq!(projected["stage"], json!([]));
}

#[cfg(feature = "bundled-render-library")]
#[test]
fn patch_cannot_hide_existing_resources_from_replacement_consent() {
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length":10,"width":10,"height":2,"body_name":"Panel"})).unwrap();
    let base = json!({"job":{"method":"gpu_path","options":{}}});
    s.execute("render.library.surface.bind", &json!({"name":"P","set":base,"surface":{"id":"SURF-001","body_index":0,"body_name":"Panel"}})).unwrap();
    let before = s.doc.to_json();
    assert!(
        s.execute(
            "render.library.surface.bind",
            &json!({"name":"P","set":{"surfaces":[]},"surface":{"id":"SURF-002","body_index":0,"body_name":"Panel"}})
        )
        .is_err()
    );
    assert_eq!(s.doc.to_json(), before);
    s.execute("render.stage.ground", &json!({"name":"P"})).unwrap();
    let before = s.doc.to_json();
    assert!(s.execute("render.stage.ground", &json!({"name":"P","set":{"stage":[]}})).is_err());
    assert!(
        s.execute(
            "render.library.studio.apply",
            &json!({"name":"P","set":{"stage":[]},"replace_lighting_and_environment":true,
        "rig":{"id":"RIG-002","variant":"complete","center_mm":[0,0,0],"subject_extent_mm":1000}})
        )
        .is_err()
    );
    assert_eq!(s.doc.to_json(), before);
}

#[cfg(not(feature = "bundled-render-library"))]
#[test]
fn omitted_optional_pack_keeps_discovery_but_refuses_resource_mutation() {
    let mut session = Session::default();
    let before = session.doc.clone();
    let history = (session.revision, session.undo.len());
    let hdr = session.execute("render.library.list", &json!({"kind":"hdr","availability":"usable"})).unwrap();
    assert_eq!(hdr["total"], 0);
    assert!(
        texture_bytes("SURF-001/base_color", MapRole::BaseColor, 16 * 1024 * 1024, "test")
            .unwrap_err()
            .to_string()
            .contains("bundled-render-library")
    );
    let request = json!({"id":"HDR-001","name":"No pack","set":{"job":{"method":"gpu_path","options":{}}}});
    assert!(session.execute("render.library.environment.apply", &request).unwrap_err().to_string().contains("bundled-render-library"));
    assert!(session.execute("render.library.surface.bind", &json!({})).unwrap_err().to_string().contains("bundled-render-library"));
    assert_eq!(session.doc, before);
    assert_eq!((session.revision, session.undo.len()), history);
}

#[cfg(feature = "bundled-render-library")]
#[test]
fn cpu_saved_surface_binding_is_offline_undoable_portable_and_preserves_geometry_and_unsupported_intent() {
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length":10,"width":20,"height":5,"body_name":"Coupon"})).unwrap();
    let before = s.doc.clone();
    let undo = s.undo.len();
    let camera = solidcraft_render::Camera::default();
    let set = json!({"camera":camera,"width":8,"height":8,"job":{"method":"cpu_raytrace","options":{"progressive":{"samples_per_pixel":3,"seed":9},"output":{"linear_exr":true}}}});
    let input = json!({"name":"CPU maps","set":set,"surface":{"id":"SURF-001","body_index":0,"body_name":"Coupon"}});
    assert!(s.render_job_host().is_none());
    s.execute("render.library.surface.bind", &input).unwrap();
    assert_eq!(s.undo.len(), undo + 1);
    assert_eq!(s.doc.features, before.features);
    assert_eq!(s.doc.materials, before.materials);
    assert_eq!(s.doc.appearances, before.appearances);
    let setup = s.doc.render.setups["CPU maps"].clone();
    assert_eq!(setup.surfaces.len(), 1);
    assert_eq!(setup.camera, camera);
    let reopened = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
    assert_eq!(reopened, *s.doc);
    let snapshot = s.doc.clone();
    let history = (s.revision, s.undo.len(), s.redo.len());
    for options in [
        json!({"reflections":{"bodies":[{"body":"Coupon","mode":"mirror"}],"samples":1,"max_bounces":2}}),
        json!({"budget":"studio","progressive":{"samples_per_pixel":3,"seed":9},"path":{"max_depth":2},"shadows":false}),
    ] {
        let mut hostile = input.clone();
        hostile["name"] = json!("Unsupported");
        hostile["set"]["job"]["options"] = options;
        assert!(s.execute("render.library.surface.bind", &hostile).is_err());
        assert!(Arc::ptr_eq(&s.doc, &snapshot));
        assert_eq!((s.revision, s.undo.len(), s.redo.len()), history);
    }
    let mut stale = setup.clone();
    let mut binding = stale.surfaces.iter().next().unwrap().clone();
    binding.source.normal_sha256 = HdrId::of(b"changed");
    stale.surfaces.set(binding, true).unwrap();
    assert!(super::super::surface_bindings::resolve(&s, &crate::product_scene::extract(&s).unwrap(), &stale.surfaces, "test").is_err());
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc, before);
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(s.doc.render.setups["CPU maps"], setup);
    s.execute("render.library.surface.unbind", &json!({"name":"CPU maps","target":setup.surfaces.iter().next().unwrap().target})).unwrap();
    assert!(s.doc.render.setups["CPU maps"].surfaces.is_empty());
    assert!(s.render_job_host().is_none());
}

#[cfg(feature = "bundled-render-library")]
#[test]
fn cpu_path_maps_bind_beside_saved_glass_and_targeted_aging_without_weakening_target_or_legacy_guards() {
    let mut s = Session::default();
    for (name, corner) in [("Glass", [0, 0, 0]), ("Panel", [20, 0, 0])] {
        s.execute("solid.box", &json!({"body_name":name,"length":10,"width":10,"height":5,"corner":corner,"operation":"new"})).unwrap();
    }
    let targets = s.execute("render.library.targets", &json!({})).unwrap();
    let glass = targets["items"].as_array().unwrap().iter().find(|target| target["name"] == "Glass").unwrap().clone();
    let panel = targets["items"].as_array().unwrap().iter().find(|target| target["name"] == "Panel").unwrap().clone();
    assert_eq!(panel["stable_target"]["definition"], "Panel");
    assert_ne!(glass["stable_target"], panel["stable_target"]);
    s.execute(
        "render.settings.set",
        &json!({"name":"Mixed","set":{"width":8,"height":8,"job":{"method":"cpu_raytrace","options":{
            "progressive":{"samples_per_pixel":4,"seed":9},"path":{"max_depth":4},
            "aging":[{"kind":"dust","target":panel["stable_target"]}]
        }}}}),
    )
    .unwrap();
    s.execute(
        "render.optics.bind",
        &json!({"name":"Mixed","body_index":glass["ordinal"],"body_name":"Glass","expected_revision":s.revision,
        "optics":{"ior":1.5,"boundary":"closed_mesh"}}),
    )
    .unwrap();
    let before = s.doc.clone();
    let history = (s.revision, s.undo.len(), s.redo.len());
    assert!(s.render_job_host().is_none(), "authoring does not require an image producer");
    let input = |target: &Value| json!({"name":"Mixed","surface":{"id":"SURF-001","body_index":target["ordinal"],"body_name":target["name"]}});
    let error = s.execute("render.library.surface.bind", &input(&glass)).unwrap_err().to_string();
    assert!(error.contains("cannot share a target"), "{error}");
    assert!(Arc::ptr_eq(&before, &s.doc));
    assert_eq!((s.revision, s.undo.len(), s.redo.len()), history);
    let mut legacy = input(&panel);
    legacy["set"] = json!({"job":{"method":"cpu_raytrace","options":{"progressive":{"samples_per_pixel":4,"seed":9}}}});
    assert!(s.execute("render.library.surface.bind", &legacy).unwrap_err().to_string().contains("require Indirect lighting"));
    assert!(Arc::ptr_eq(&before, &s.doc));
    assert_eq!((s.revision, s.undo.len(), s.redo.len()), history);
    s.execute("render.library.surface.bind", &input(&panel)).unwrap();
    assert_eq!(s.undo.len(), history.1 + 1);
    let saved = s.doc.clone();
    let setup = &saved.render.setups["Mixed"];
    assert_eq!(setup.surfaces.len(), 1);
    assert_eq!(setup.optics, before.render.setups["Mixed"].optics);
    let solidcraft_doc::render_settings::RenderJob::CpuRaytrace { options, .. } = setup.job.as_ref().unwrap() else { panic!("CPU intent changed") };
    assert!(options.path.is_some());
    assert_eq!(options.aging.iter().next().unwrap().target.as_ref().unwrap(), &setup.surfaces.iter().next().unwrap().target);
    assert_eq!(saved.features, before.features);
    assert_eq!(saved.materials, before.materials);
    assert_eq!(saved.appearances, before.appearances);
    assert_eq!(solidcraft_doc::Document::from_json(&saved.to_json()).unwrap(), *saved);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc, before);
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(s.doc, saved);
    assert!(s.render_job_host().is_none());
}

#[test]
fn cpu_authored_rig_domain_is_exact_and_preserves_dormant_sampling_without_implicit_lights() {
    use solidcraft_doc::render_settings::CpuJob;
    let mut s = Session::default();
    let mut accepted = Vec::new();
    let mut over_domain = Vec::new();
    for index in 1..=24 {
        let id = format!("RIG-{index:03}");
        let rig = json!({"id":id,"variant":"complete","center_mm":[0,0,0],"subject_extent_mm":100});
        if s.execute("render.library.studio.prepare", &rig).is_err() {
            continue;
        }
        let mut prepare = rig.clone();
        prepare["name"] = json!("Rig");
        prepare["set"] = json!({"width":8,"height":8,"job":{"method":"cpu_raytrace","options":{"area_samples":64,"seed":731}}});
        let before = s.doc.clone();
        let history = s.undo.len();
        let result = s.execute("render.library.studio.prepare", &prepare);
        assert!(Arc::ptr_eq(&before, &s.doc));
        assert_eq!(history, s.undo.len());
        match result {
            Ok(plan) => {
                accepted.push(id.clone());
                let options: CpuJob = serde_json::from_value(plan["setup"]["job"]["options"].clone()).unwrap();
                assert_eq!(options.lights, Some(vec![]));
                assert_eq!(options.punctual_lights, Some(vec![]));
                assert_eq!((options.area_samples, options.seed), (Some(64), Some(731)));
                assert!(options.environment.is_none() && options.sky.is_none() && options.hdr.is_none());
                assert_eq!(plan["requires_indirect"], id == "RIG-020");
            }
            Err(error) => {
                assert!(error.to_string().contains("exceeds CPU area intensity 0..64"), "{id}: {error}");
                over_domain.push(id);
                let err = s
                    .execute(
                        "render.library.studio.apply",
                        &json!({"rig":rig,"name":"Rig","set":prepare["set"],"replace_lighting_and_environment":true}),
                    )
                    .unwrap_err();
                assert!(err.to_string().contains("exceeds CPU area intensity 0..64"));
                assert!(Arc::ptr_eq(&before, &s.doc));
                assert_eq!(history, s.undo.len());
            }
        }
    }
    assert_eq!(accepted, vec!["RIG-007", "RIG-020", "RIG-022"]);
    assert_eq!(over_domain.len(), 16);
    let mut options = CpuJob { area_samples: Some(64), seed: Some(731), ..Default::default() };
    let before = options.clone();
    let light = AreaLight { center: [0., 0., 10.], u: [1., 0., 0.], v: [0., -1., 0.], radiance: [1., 0.5, 0.25] };
    let lighting = StudioLighting { area_lights: AreaLights::new(&[light]).unwrap(), ..Default::default() };
    assert!(cpu_studio_lighting(&mut options, &lighting, "test").unwrap_err().to_string().contains("lossless CPU colour/intensity projection"));
    assert_eq!(options, before, "no lossy colour decomposition");
}

#[test]
fn cpu_sun_rig_adoption_is_explicit_complete_atomic_and_one_undo() {
    let mut s = Session::default();
    s.execute("render.settings.set",&json!({"name":"Rig","set":{"width":8,"height":8,"job":{"method":"cpu_raytrace","options":{"primary_samples":4,"shadows":false,"area_samples":64,"seed":731}}}})).unwrap();
    let rig = json!({"id":"RIG-020","variant":"complete","center_mm":[0,0,0],"subject_extent_mm":100});
    let mut prepare = rig.clone();
    prepare["name"] = json!("Rig");
    let before = s.doc.clone();
    let undo = s.undo.len();
    let plan = s.execute("render.library.studio.prepare", &prepare).unwrap();
    assert_eq!(plan["requires_indirect"], true);
    assert!(Arc::ptr_eq(&before, &s.doc));
    let mut apply = json!({"name":"Rig","rig":rig,"replace_lighting_and_environment":true,"expected_revision":s.revision});
    assert!(s.execute("render.library.studio.apply", &apply).unwrap_err().to_string().contains("Enable indirect lighting"));
    assert_eq!(s.doc, before);
    assert_eq!(s.undo.len(), undo);
    apply["enable_indirect"] = json!(true);
    s.execute("render.library.studio.apply", &apply).unwrap();
    assert_eq!(s.undo.len(), undo + 1);
    let Some(RenderJob::CpuRaytrace { options, .. }) = &s.doc.render.setups["Rig"].job else { panic!() };
    assert_eq!(options.path.unwrap().max_depth, 8);
    assert_eq!(options.progressive.unwrap().samples_per_pixel, 16);
    assert_eq!((options.primary_samples, options.shadows, options.area_samples, options.seed), (1, true, Some(64), Some(731)));
    assert!(options.sun.is_some());
    assert_eq!(options.area_lights, Some(vec![]));
    let saved = s.doc.to_json();
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc, before);
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(s.doc.to_json(), saved);
    let before = s.doc.clone();
    assert!(s.execute("render.library.studio.apply", &apply).is_err());
    assert!(Arc::ptr_eq(&before, &s.doc));
    let mut large = Session::default();
    let fixed = json!({"width":2048,"height":2048,"job":{"method":"cpu_raytrace","options":{}}});
    // This is an admitted fixed Compact image. Only explicit adoption of the
    // complete progressive16 Sun request crosses Compact's1,048,576-pixel bound.
    // Upstream's shared setup refusal names the admitting Studio profile rather
    // than repeating the current Compact name, so assert the actual size proof.
    use solidcraft_render::job::{CpuBudget, CpuProgressiveOptions};
    let adopted = Some(CpuProgressiveOptions { samples_per_pixel: 16, seed: 0 });
    assert!(CpuBudget::Compact.validate_image(2048, 2048, None).is_ok());
    assert_eq!(CpuBudget::Compact.limits().max_progressive_pixels, 1_048_576);
    assert!(CpuBudget::Compact.validate_image(2048, 2048, adopted).is_err());
    assert!(CpuBudget::Studio.validate_image(2048, 2048, adopted).is_ok());
    let authored = super::super::patched_settings(&large, "Rig", &fixed, "render.library.studio.prepare").unwrap();
    assert_eq!((authored.width, authored.height), (2048, 2048));
    let Some(RenderJob::CpuRaytrace { options, .. }) = &authored.job else { panic!("typed fixed CPU setup") };
    assert_eq!(options.budget, CpuBudget::Compact);
    assert!(options.progressive.is_none() && options.path.is_none());
    let mut oversized = prepare;
    oversized["set"] = fixed.clone();
    let before = large.doc.clone();
    let state = (large.revision, large.evaluations, large.is_dirty(), large.undo.len(), large.redo.len(), large.camera);
    let check_size_error = |error: String| {
        assert!(error.contains("render output 2048 × 2048 exceeds the image limits"), "{error}");
        assert!(error.contains("1048576 pixels in all"), "{error}");
        assert!(error.contains("largest image at this aspect is 1024 × 1024"), "{error}");
        assert!(error.contains("choose the Studio CPU resource profile, or a smaller size"), "{error}");
        eprintln!("complete CPU Sun adoption refused without mutation: {error}");
    };
    check_size_error(large.execute("render.library.studio.prepare", &oversized).unwrap_err().to_string());
    assert!(Arc::ptr_eq(&before, &large.doc));
    assert_eq!((large.revision, large.evaluations, large.is_dirty(), large.undo.len(), large.redo.len(), large.camera), state);
    let mut oversized = apply;
    oversized.as_object_mut().unwrap().remove("expected_revision");
    oversized["set"] = fixed;
    check_size_error(large.execute("render.library.studio.apply", &oversized).unwrap_err().to_string());
    assert!(Arc::ptr_eq(&before, &large.doc));
    assert_eq!((large.revision, large.evaluations, large.is_dirty(), large.undo.len(), large.redo.len(), large.camera), state);
    assert!(large.undo.is_empty());
}

#[test]
fn cpu_sun_rig_keeps_existing_path_adaptive_seed_and_profile_and_rejects_unknown_context() {
    let mut s = Session::default();
    let options = json!({"budget":"studio","progressive":{"samples_per_pixel":64,"seed":947},"adaptive":{"pilot_samples":32,"min_samples":32},"path":{"max_depth":5},"area_samples":4,"seed":731});
    s.execute("render.settings.set", &json!({"name":"Rig","set":{"width":96,"height":64,"job":{"method":"cpu_raytrace","options":options}}}))
        .unwrap();
    let rig = json!({"id":"RIG-020","variant":"complete","center_mm":[0,0,0],"subject_extent_mm":100});
    let mut prepare = rig.clone();
    prepare["name"] = json!("Rig");
    let plan = s.execute("render.library.studio.prepare", &prepare).unwrap();
    assert_eq!(plan["requires_indirect"], false);
    let old = s.doc.render.setups["Rig"].clone();
    s.execute("render.library.studio.apply", &json!({"name":"Rig","rig":rig,"replace_lighting_and_environment":true})).unwrap();
    let Some(RenderJob::CpuRaytrace { options: before, .. }) = old.job else { panic!() };
    let Some(RenderJob::CpuRaytrace { options: after, .. }) = &s.doc.render.setups["Rig"].job else { panic!() };
    assert_eq!(
        (after.budget, after.progressive, after.adaptive, after.path, after.area_samples, after.seed),
        (before.budget, before.progressive, before.adaptive, before.path, before.area_samples, before.seed)
    );
    let doc = s.doc.clone();
    let undo = s.undo.len();
    prepare["extra"] = json!(true);
    assert!(s.execute("render.library.studio.prepare", &prepare).is_err());
    assert!(Arc::ptr_eq(&doc, &s.doc));
    assert_eq!(s.undo.len(), undo);
}
