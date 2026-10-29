use super::*;
use solidcraft_doc::render_settings::RenderJob;

fn save(s: &mut Session, patch: Value) {
    s.execute("render.settings.set", &json!({"name":"Studio", "set":patch})).unwrap();
}

#[test]
fn environment_editor_rotation_wraps_and_each_edit_is_one_undo() {
    let mut s = Session::default();
    save(&mut s, json!({"job":{"method":"gpu_path","options":{"environment":{"kind":"sky"}}}}));
    for (degrees, expected) in [(215.0, -145.0), (540.0, -180.0), (-541.0, 179.0), (1e6, -80.0), (-1e6, 80.0)] {
        let before = s.doc.clone();
        let undo = s.undo.len();
        let result = s.execute("render.environment.set", &json!({"name":"Studio","rotation_deg":degrees})).unwrap();
        let angle = result["setup"]["job"]["options"]["environment"]["rotation_rad"].as_f64().unwrap().to_degrees();
        assert!((-180.0..180.0).contains(&angle));
        assert!((angle - expected).abs() < 1e-9);
        assert_eq!(s.undo.len(), undo + 1);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc, before);
        s.execute("edit.redo", &json!({})).unwrap();
    }
    let undo = s.undo.len();
    s.execute("render.environment.set", &json!({"name":"Studio","intensity":2.5})).unwrap();
    assert_eq!(s.undo.len(), undo + 1);
    s.execute("render.environment.set", &json!({"name":"Studio","intensity":2.5})).unwrap();
    assert_eq!(s.undo.len(), undo + 1, "unchanged values do not add history");
}

#[test]
fn environment_editor_hostile_inputs_refuse_atomically() {
    let mut s = Session::default();
    save(&mut s, json!({"job":{"method":"gpu_path","options":{"environment":{"kind":"sky"}}}}));
    let before = s.doc.clone();
    let history = (s.revision, s.undo.len(), s.redo.len());
    for extra in [
        json!({"rotation_deg":1e6+1.0}),
        json!({"rotation_deg":-1e6-1.0}),
        json!({"rotation_deg":1e300}),
        json!({"rotation_deg":"NaN"}),
        json!({"rotation_deg":null}),
        json!({"intensity":-0.01}),
        json!({"intensity":100.01}),
        json!({"intensity":1e300}),
        json!({"intensity":"Infinity"}),
        json!({"rotation_deg":30,"intensity":101}),
        json!({"unexpected":true}),
        json!({}),
    ] {
        let mut request = json!({"name":"Studio"});
        request.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        assert!(s.execute("render.environment.set", &request).is_err(), "{request}");
        assert!(Arc::ptr_eq(&before, &s.doc));
        assert_eq!(history, (s.revision, s.undo.len(), s.redo.len()));
    }
}

#[test]
fn environment_editor_preserves_source_and_refuses_absent_or_incompatible_environment() {
    let mut s = Session::default();
    save(&mut s, json!({"job":{"method":"cpu_raytrace","options":{"environment":{"color":[0.2,0.3,0.4],"intensity":1}}}}));
    let before = s.doc.clone();
    for request in [json!({"rotation_deg":30}), json!({"intensity":64.01})] {
        let mut params = json!({"name":"Studio"});
        params.as_object_mut().unwrap().extend(request.as_object().unwrap().clone());
        assert!(s.execute("render.environment.set", &params).is_err());
        assert!(Arc::ptr_eq(&before, &s.doc));
    }
    let reply = s.execute("render.environment.set", &json!({"name":"Studio","intensity":64})).unwrap();
    assert_eq!(reply["setup"]["job"]["options"]["environment"]["color"], json!([0.2, 0.3, 0.4]));
    for job in [json!({"method":"raster"}), json!({"method":"gpu_path"}), json!({"method":"cpu_raytrace"})] {
        let before = s.doc.clone();
        assert!(s.execute("render.environment.set", &json!({"name":"Studio","set":{"job":job},"intensity":2})).is_err());
        assert!(Arc::ptr_eq(&before, &s.doc));
    }
}

#[test]
fn environment_editor_command_has_schemas_and_strict_validation() {
    let command = crate::find_command("render.environment.set").unwrap();
    assert!(command.contract.is_some());
    assert!(command.output.is_some());
    let mut s = Session { param_check: crate::schema::ParamCheck::Strict, ..Default::default() };
    save(&mut s, json!({"job":{"method":"gpu_path","options":{"environment":{"kind":"sky"}}}}));
    let reply = s.execute("render.environment.set", &json!({"name":"Studio","intensity":2})).unwrap();
    assert_eq!(reply["setup"]["job"]["options"]["environment"]["intensity"], 2.0);
    assert!(s.execute("render.environment.set", &json!({"name":"Studio","intensity":2,"unknown":true})).is_err());
}

#[test]
fn shot_frame_edits_are_bounded_atomic_and_one_undo_per_edit() {
    let mut s = Session::default();
    save(&mut s, json!({"width":800,"height":600}));
    for patch in [
        json!({"width":1920,"height":1080}),
        json!({"width":1080,"height":1920}),
        json!({"exposure_ev":-32}),
        json!({"exposure_ev":32}),
        json!({"camera":{"target":[0,0,0],"yaw":0,"pitch":0,"distance":1e9,"fov":0}}),
    ] {
        let before = s.doc.clone();
        let undo = s.undo.len();
        let evaluations = s.evaluations;
        save(&mut s, patch.clone());
        assert_eq!(s.undo.len(), undo + 1);
        assert_eq!(s.evaluations, evaluations);
        save(&mut s, patch);
        assert_eq!(s.undo.len(), undo + 1);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc, before);
    }
    let before = s.doc.clone();
    let undo = s.undo.len();
    for patch in [
        json!({"width":0}),
        json!({"height":0}),
        json!({"width":u32::MAX}),
        json!({"width":4096,"height":4096}),
        json!({"width":1e300}),
        json!({"exposure_ev":32.1}),
        json!({"exposure_ev":-32.1}),
        json!({"exposure_ev":null}),
        json!({"camera":{"target":[0,0,0],"yaw":0,"pitch":0,"distance":0,"fov":0}}),
    ] {
        assert!(s.execute("render.settings.set", &json!({"name":"Studio","set":patch})).is_err());
        assert!(Arc::ptr_eq(&s.doc, &before));
        assert_eq!(s.undo.len(), undo);
    }
}

#[test]
fn render_lights_visibility_commands_validate_and_echo_without_document_or_history_changes() {
    let mut s = Session::default();
    save(&mut s, json!({"width":800}));
    save(&mut s, json!({"width":900}));
    s.execute("edit.undo", &json!({})).unwrap();
    s.mark_saved();
    let document = s.doc.clone();
    let before = (s.undo.len(), s.redo.len(), s.revision, s.evaluations, s.is_dirty());
    for (command, input) in [
        ("render.lights.solo", json!({"id":"directional:0","solo":true})),
        ("render.lights.solo", json!({"solo":false})),
        ("render.lights.mute", json!({"id":"area:3","mute":true})),
        ("render.lights.mute", json!({"id":"spot:7","mute":false})),
    ] {
        let reply = s.execute(command, &input).unwrap();
        for (key, value) in input.as_object().unwrap() {
            assert_eq!(reply.get(key), Some(value));
        }
        let spec = crate::find_command(command).unwrap();
        assert!(!spec.undoable && !spec.reads_model);
        let description = s.execute("engine.describe", &json!({"command":command})).unwrap();
        assert!(description["input"].is_object() && description["output"].is_object());
    }
    for (command, input) in [
        ("render.lights.solo", json!({"solo":true})),
        ("render.lights.solo", json!({"id":"point:8","solo":true})),
        ("render.lights.solo", json!({"id":"point:00","solo":true})),
        ("render.lights.solo", json!({"id":"directional:18446744073709551615","solo":true})),
        ("render.lights.solo", json!(["area:0", true])),
        ("render.lights.solo", json!({"id":"area:0","solo":"true"})),
        ("render.lights.mute", json!({"id":"area:4","mute":true})),
        ("render.lights.mute", json!({"id":"unknown:0","mute":true})),
        ("render.lights.mute", json!({"id":"area:0","mute":true,"save":true})),
        ("render.lights.mute", json!({"mute":true})),
    ] {
        assert!(s.execute(command, &input).is_err(), "{command}: {input}");
    }
    assert!(Arc::ptr_eq(&document, &s.doc));
    assert_eq!(before, (s.undo.len(), s.redo.len(), s.revision, s.evaluations, s.is_dirty()));
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(s.doc.render.setups["Studio"].width, 900);
}

fn light_fixture(s: &mut Session, method: &str) {
    let options = if method == "gpu_path" { json!({"lighting":{}}) } else { json!({"lights":[],"punctual_lights":[],"area_lights":[]}) };
    save(s, json!({"job":{"method":method,"options":options}}));
}

#[test]
fn render_lights_small_area_pose_and_resize_preserve_aspect_and_emitting_side() {
    use crate::schema::render_lights::Geometry;
    use solidcraft_geom::Vec3;
    let mut s = Session::default();
    save(
        &mut s,
        json!({"job":{"method":"cpu_raytrace","options":{"lights":[],"area_lights":[{
            "center_mm":[0,0,10],"edge_u_mm":[0.000002,0,0],"edge_v_mm":[0,-0.000004,0],"color":[1,1,1],"intensity":1
        }]}}}),
    );
    s.execute("render.lights.set", &json!({"name":"Studio","id":"area:0","set":{"azimuth_deg":90,"elevation_deg":45}})).unwrap();
    let before = crate::render_lights::inspect(&s.doc.render.setups["Studio"]).unwrap();
    let Geometry::Area { edge_u_mm: [ux, uy, uz], edge_v_mm: [vx, vy, vz], .. } = before[0].geometry else { panic!("area") };
    let u = Vec3::new(ux, uy, uz);
    let v = Vec3::new(vx, vy, vz);
    assert!((u.len() - 0.000002).abs() < 1e-12);
    assert!((v.len() / u.len() - 2.0).abs() < 1e-12);
    s.execute("render.lights.set", &json!({"name":"Studio","id":"area:0","set":{"size_mm":20}})).unwrap();
    let after = crate::render_lights::inspect(&s.doc.render.setups["Studio"]).unwrap();
    let Geometry::Area { edge_u_mm: [ux, uy, uz], edge_v_mm: [vx, vy, vz], .. } = after[0].geometry else { panic!("area") };
    let resized_u = Vec3::new(ux, uy, uz);
    let resized_v = Vec3::new(vx, vy, vz);
    assert!((resized_u.len() - 20.0).abs() < 1e-10);
    assert!((resized_v.len() - 40.0).abs() < 1e-10);
    assert!(
        solidcraft_render::surface::unit_direction(u.cross(v))
            .unwrap()
            .dot(solidcraft_render::surface::unit_direction(resized_u.cross(resized_v)).unwrap())
            > 0.999999
    );
}

#[test]
fn render_lights_edit_commands_publish_one_undo_and_preserve_other_settings() {
    let mut s = Session::default();
    light_fixture(&mut s, "cpu_raytrace");
    for (command, params) in [
        ("render.lights.add", json!({"name":"Studio","set":{"kind":"point"}})),
        (
            "render.lights.set",
            json!({"name":"Studio","id":"point:0","set":{"intensity_ev":2,"temperature_k":3200,"azimuth_deg":35,"elevation_deg":45,"distance_mm":100}}),
        ),
        ("render.lights.duplicate", json!({"name":"Studio","id":"point:0"})),
        ("render.lights.set", json!({"name":"Studio","id":"point:1","set":{"kind":"spot","aim_at":[0,0,0]}})),
        ("render.lights.remove", json!({"name":"Studio","id":"point:0"})),
    ] {
        let before = s.doc.clone();
        let undo = s.undo.len();
        let reply = s.execute(command, &params).unwrap();
        assert_eq!(s.undo.len(), undo + 1, "{command}");
        assert_eq!(reply["setup"]["width"], 1024);
        let changed = s.doc.clone();
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc, before, "{command}");
        s.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(s.doc, changed, "{command}");
    }
    let list = s.execute("render.lights.list", &json!({"setup":"Studio"})).unwrap();
    assert_eq!(list["lights"].as_array().unwrap().len(), 1);
    assert_eq!(list["lights"][0]["kind"], "spot");
    assert_eq!(list["lights"][0]["intensity"], 4.0);
    assert_eq!(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap(), *s.doc);
}

#[test]
fn render_lights_edit_refusals_are_atomic_for_hostile_numbers_ids_stale_reviews_and_caps() {
    let mut s = Session::default();
    light_fixture(&mut s, "cpu_raytrace");
    s.execute("render.lights.add", &json!({"name":"Studio","set":{"kind":"point"}})).unwrap();
    for input in [
        json!({"name":"Studio","id":"point:0","set":{"intensity_ev":1e300}}),
        json!({"name":"Studio","id":"point:0","set":{"intensity_ev":7}}),
        json!({"name":"Studio","id":"point:0","set":{"temperature_k":999}}),
        json!({"name":"Studio","id":"point:0","set":{"softness":0.5}}),
        json!({"name":"Studio","id":"point:0","set":{"aim_at":[1e300,0,0]}}),
        json!({"name":"Studio","id":"point:0","set":{"distance_mm":-1}}),
        json!({"name":"Studio","id":"point:0","set":{"kind":"invalid"}}),
        json!({"name":"Studio","id":"area:0","set":{"intensity_ev":1}}),
        json!({"name":"Studio","id":"point:0","set":{"intensity_ev":1},"expected_revision":0}),
        json!({"name":"Studio","id":"point:0","set":{"save":true}}),
    ] {
        let document = s.doc.clone();
        let before = (s.undo.len(), s.redo.len(), s.revision, s.evaluations);
        assert!(s.execute("render.lights.set", &input).is_err(), "{input}");
        assert!(Arc::ptr_eq(&document, &s.doc));
        assert_eq!(before, (s.undo.len(), s.redo.len(), s.revision, s.evaluations));
    }
    for (kind, cap) in [("directional", 8), ("point", 8), ("area", 4)] {
        light_fixture(&mut s, "cpu_raytrace");
        for _ in 0..cap {
            s.execute("render.lights.add", &json!({"name":"Studio","set":{"kind":kind}})).unwrap();
        }
        let document = s.doc.clone();
        let before = (s.undo.len(), s.revision);
        assert!(s.execute("render.lights.add", &json!({"name":"Studio","set":{"kind":kind}})).is_err());
        assert!(s.execute("render.lights.duplicate", &json!({"name":"Studio","id":format!("{kind}:0")})).is_err());
        assert!(Arc::ptr_eq(&document, &s.doc));
        assert_eq!(before, (s.undo.len(), s.revision));
    }
    light_fixture(&mut s, "gpu_path");
    s.execute("render.lights.add", &json!({"name":"Studio","set":{"kind":"directional"}})).unwrap();
    let document = s.doc.clone();
    for command in ["render.lights.add", "render.lights.duplicate"] {
        let params =
            if command.ends_with("add") { json!({"name":"Studio","set":{"kind":"point"}}) } else { json!({"name":"Studio","id":"directional:0"}) };
        assert!(s.execute(command, &params).is_err());
        assert!(Arc::ptr_eq(&document, &s.doc));
    }
    for _ in 0..4 {
        s.execute("render.lights.add", &json!({"name":"Studio","set":{"kind":"area"}})).unwrap();
    }
    assert!(s.execute("render.lights.add", &json!({"name":"Studio","set":{"kind":"area"}})).is_err());
}

#[test]
fn render_lights_filter_copies_preserve_saved_settings_and_environment() {
    use crate::schema::render_lights::Visibility;
    let mut s = Session::default();
    light_fixture(&mut s, "gpu_path");
    s.execute("render.lights.add", &json!({"name":"Studio","set":{"kind":"area"}})).unwrap();
    s.execute("render.lights.duplicate", &json!({"name":"Studio","id":"area:0"})).unwrap();
    let original = s.doc.render.setups["Studio"].clone();
    let mut copy = original.clone();
    let mut visibility = Visibility { solo: Some("area:1".into()), ..Default::default() };
    assert_eq!(crate::render_lights::filter(&mut copy, &visibility).unwrap(), None);
    let lights = crate::render_lights::inspect(&copy).unwrap();
    assert_eq!(lights[0].intensity, 0.0);
    assert!(lights[1].intensity > 0.0);
    visibility.muted.insert("area:1".into());
    crate::render_lights::filter(&mut copy, &visibility).unwrap();
    assert!(crate::render_lights::inspect(&copy).unwrap().iter().all(|light| light.intensity == 0.0));
    assert_eq!(s.doc.render.setups["Studio"], original);
    let mut raster = RenderSetup::default();
    let unchanged = raster.clone();
    visibility.solo = Some("directional:1".into());
    visibility.muted.clear();
    assert_eq!(crate::render_lights::filter(&mut raster, &visibility).unwrap(), Some([0.0, 1.0, 0.0]));
    assert_eq!(raster, unchanged);
}

#[test]
fn render_lights_list_unifies_cpu_families_and_keeps_source_slot_ids() {
    let mut s = Session::default();
    let directional = json!({"direction":[0,0,2],"color":[1,0.5,0.25],"intensity":3});
    let point = json!({"position_mm":[2,3,4],"color":[0.2,0.4,1],"intensity":2,"reference_distance_mm":10,"minimum_distance_mm":0.01});
    let mut spot = point.clone();
    spot["cone"] = json!({"emitted_axis":[0,0,-1],"inner_half_angle_radians":0.2,"outer_half_angle_radians":0.5});
    let area = json!({"center_mm":[0,0,10],"edge_u_mm":[4,0,0],"edge_v_mm":[0,-6,0],"color":[1,1,1],"intensity":4});
    save(
        &mut s,
        json!({"job":{"method":"cpu_raytrace","options":{
        "lights":[directional],"punctual_lights":[point,spot],"area_lights":[area]}}}),
    );
    let result = s.execute("render.lights.list", &json!({"setup":"Studio"})).unwrap();
    let lights = result["lights"].as_array().unwrap();
    assert_eq!(lights.len(), 4);
    for (light, (id, kind, name)) in lights.iter().zip([
        ("directional:0", "directional", "Key"),
        ("point:0", "point", "Fill"),
        ("spot:1", "spot", "Rim"),
        ("area:0", "area", "Light 4"),
    ]) {
        assert_eq!(light["id"], id);
        assert_eq!(light["kind"], kind);
        assert_eq!(light["name"], name);
        assert_eq!(light["implicit"], false);
    }
    assert_eq!(lights[0]["direction"], json!([0.0, 0.0, 2.0]));
    assert_eq!(lights[0]["colour"], json!([1.0, 0.5, 0.25]));
    assert_eq!(lights[1]["minimum_distance_mm"], 0.01);
    assert_eq!(lights[2]["outer_half_angle_radians"], 0.5);
    assert_eq!(lights[3]["edge_v_mm"], json!([0.0, -6.0, 0.0]));
    assert_eq!(lights[3]["name_key"], "render.light_default_number");
    assert_eq!(lights[3]["number"], 4);
    let mut setup = serde_json::to_value(&s.doc.render.setups["Studio"]).unwrap();
    setup["job"]["options"]["lights"][0]["intensity"] = json!(6);
    let document = s.doc.clone();
    let edited = s.execute("render.lights.list", &json!({"settings":setup})).unwrap();
    assert_eq!(edited["lights"][0]["id"], lights[0]["id"]);
    assert_eq!(edited["lights"][0]["intensity"], 6.0);
    assert!(Arc::ptr_eq(&document, &s.doc));
}

#[test]
fn render_lights_list_factors_gpu_radiance_and_converts_half_edges() {
    let mut s = Session::default();
    let result = s
        .execute(
            "render.lights.list",
            &json!({"settings":{"job":{"method":"gpu_path","options":{"lighting":{
                "sun":{"direction":[0,0,1],"angular_radius_rad":0.02,"radiance":[1000,500,250]},
                "area_lights":[{"center":[2,3,4],"u":[5,0,0],"v":[0,-6,0],"radiance":[8,4,2]},
                               {"center":[0,0,10],"u":[1,0,0],"v":[0,1,0],"radiance":[0,0,0]}]
            }}}}}),
        )
        .unwrap();
    let lights = result["lights"].as_array().unwrap();
    assert_eq!(lights.len(), 3);
    assert_eq!(lights[0]["id"], "directional:0");
    assert_eq!(lights[0]["angular_radius_rad"], 0.02);
    assert_eq!(lights[0]["intensity"], 1000.0);
    assert_eq!(lights[0]["colour"], json!([1.0, 0.5, 0.25]));
    assert_eq!(lights[1]["id"], "area:0");
    assert_eq!(lights[1]["position_mm"], json!([2.0, 3.0, 4.0]));
    assert_eq!(lights[1]["edge_u_mm"], json!([10.0, 0.0, 0.0]));
    assert_eq!(lights[1]["edge_v_mm"], json!([0.0, -12.0, 0.0]));
    for (light, expected) in lights.iter().zip([[1000.0, 500.0, 250.0], [8.0, 4.0, 2.0], [0.0; 3]]) {
        let intensity = light["intensity"].as_f64().unwrap();
        for (colour, radiance) in light["colour"].as_array().unwrap().iter().zip(expected) {
            assert_eq!(colour.as_f64().unwrap() * intensity, radiance);
        }
    }
}

#[test]
fn render_lights_list_distinguishes_implicit_presets_empty_rigs_and_environments() {
    let mut s = Session::default();
    for (settings, count, implicit) in [
        (json!({}), 3, true),
        (json!({"job":{"method":"raster"}}), 3, true),
        (json!({"job":{"method":"cpu_raytrace"}}), 3, true),
        (json!({"job":{"method":"cpu_raytrace","options":{"preset":"warm_cool"}}}), 3, true),
        (json!({"job":{"method":"cpu_raytrace","options":{"preset":"key_rim"}}}), 2, true),
        (json!({"job":{"method":"cpu_raytrace","options":{"lights":[]}}}), 0, false),
        (json!({"job":{"method":"gpu_path"}}), 3, true),
        (json!({"job":{"method":"gpu_path","options":{"lighting":{}}}}), 0, false),
        (json!({"job":{"method":"gpu_path","options":{"environment":{"kind":"sky"}}}}), 0, false),
    ] {
        let reply = s.execute("render.lights.list", &json!({"settings":settings})).unwrap();
        let lights = reply["lights"].as_array().unwrap();
        assert_eq!(lights.len(), count, "{settings}");
        assert!(lights.iter().all(|light| light["implicit"] == implicit));
    }
    let warm = s.execute("render.lights.list", &json!({"settings":{"job":{"method":"cpu_raytrace","options":{"preset":"warm_cool"}}}})).unwrap();
    assert_eq!(warm["lights"][0]["colour"], json!([1.0, 0.72, 0.48]));
}

#[test]
fn render_lights_list_accepts_all_twenty_cpu_slots_and_refuses_over_caps() {
    let mut s = Session::default();
    let directional = json!({"direction":[0,0,1],"color":[1,1,1],"intensity":1});
    let point = json!({"position_mm":[0,0,10],"color":[1,1,1],"intensity":1,"reference_distance_mm":10,"minimum_distance_mm":0.01});
    let area = json!({"center_mm":[0,0,10],"edge_u_mm":[4,0,0],"edge_v_mm":[0,6,0],"color":[1,1,1],"intensity":1});
    let mut settings = json!({"job":{"method":"cpu_raytrace","options":{
        "lights":vec![directional.clone();8],"punctual_lights":vec![point.clone();8],"area_lights":vec![area.clone();4]}}});
    let reply = s.execute("render.lights.list", &json!({"settings":settings})).unwrap();
    let lights = reply["lights"].as_array().unwrap();
    assert_eq!(lights.len(), 20);
    assert_eq!(lights.last().unwrap()["id"], "area:3");
    let ids: std::collections::BTreeSet<_> = lights.iter().map(|l| l["id"].as_str().unwrap()).collect();
    assert_eq!(ids.len(), 20);
    for (field, value) in [("lights", directional), ("punctual_lights", point), ("area_lights", area)] {
        settings["job"]["options"][field].as_array_mut().unwrap().push(value);
        assert!(s.execute("render.lights.list", &json!({"settings":settings})).is_err());
        settings["job"]["options"][field].as_array_mut().unwrap().pop();
    }
    let area = json!({"center":[0,0,10],"u":[1,0,0],"v":[0,1,0],"radiance":[1,1,1]});
    for count in [4, 5] {
        let result = s.execute(
            "render.lights.list",
            &json!({"settings":{"job":{"method":"gpu_path","options":{
            "lighting":{"area_lights":vec![area.clone();count]}}}}}),
        );
        if count == 4 {
            assert_eq!(result.unwrap()["lights"].as_array().unwrap().len(), 4);
        } else {
            assert!(result.is_err());
        }
    }
}

#[test]
fn render_lights_list_hostile_calls_and_reads_preserve_document_history_and_dirty_state() {
    let mut s = Session::default();
    save(&mut s, json!({"width":800}));
    save(&mut s, json!({"width":900}));
    s.execute("edit.undo", &json!({})).unwrap();
    s.mark_saved();
    let document = s.doc.clone();
    let before = (s.undo.len(), s.redo.len(), s.revision, s.evaluations, s.is_dirty());
    for params in [
        json!([]),
        json!([null, null]),
        json!(3),
        json!({"unknown":true}),
        json!({"setup":null}),
        json!({"settings":null}),
        json!({"setup":[]}),
        json!({"setup":"missing"}),
        json!({"setup":"Studio","settings":{}}),
        json!({"settings":[]}),
        json!({"settings":{"job":{"method":"cpu_raytrace","options":{"lights":[{
            "direction":[0,0,0],"color":[1,1,1],"intensity":1}]}}}}),
        json!({"settings":{"job":{"method":"gpu_path","options":{"lighting":{"sun":{
            "direction":[0,0,1],"angular_radius_rad":0.02,"radiance":[1e300,0,0]}}}}}}),
    ] {
        assert!(s.execute("render.lights.list", &params).is_err(), "{params}");
        assert!(Arc::ptr_eq(&document, &s.doc));
        assert_eq!(before, (s.undo.len(), s.redo.len(), s.revision, s.evaluations, s.is_dirty()));
    }
    // Session::execute normalizes a top-level null to the empty parameter object.
    for params in [Value::Null, json!({}), json!({"setup":"Studio"}), json!({"settings":{"job":{"method":"cpu_raytrace"}}})] {
        s.execute("render.lights.list", &params).unwrap();
        assert!(Arc::ptr_eq(&document, &s.doc));
        assert_eq!(before, (s.undo.len(), s.redo.len(), s.revision, s.evaluations, s.is_dirty()));
    }
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(s.doc.render.setups["Studio"].width, 900);
}

#[test]
fn render_lights_list_publishes_and_checks_both_schemas() {
    let mut s = Session::default();
    let reply = s.execute("engine.describe", &json!({"command":"render.lights.list"})).unwrap();
    assert_eq!(reply["input"]["properties"].as_object().unwrap().keys().map(String::as_str).collect::<Vec<_>>(), ["settings", "setup"]);
    assert!(reply["input"]["properties"].get("setup").is_some());
    assert!(reply["input"]["properties"].get("settings").is_some());
    assert!(reply["output"]["properties"].get("lights").is_some());
    for mode in [crate::schema::ParamCheck::Off, crate::schema::ParamCheck::Warn, crate::schema::ParamCheck::Strict] {
        s.param_check = mode;
        assert!(s.execute("render.lights.list", &json!({"typo":true})).is_err());
        let reply = s.execute("render.lights.list", &json!({})).unwrap();
        assert_eq!(reply["lights"].as_array().unwrap().len(), 3);
    }
}

#[test]
fn render_lights_list_does_not_settle_a_stale_model() {
    let mut s = Session::default();
    for (command, params) in [
        ("sketch.create", json!({"plane":"XY"})),
        ("sketch.rectangle.two_point", json!({"p0":[5,5],"p1":[25,20]})),
        ("sketch.finish", json!({})),
        ("solid.extrude", json!({"distance":5})),
        ("sketch.edit", json!({"sketch":"Sketch1"})),
        ("sketch.drag", json!({"entity":"l1.start","from":[5,5],"to":[4,5],"live":true})),
    ] {
        s.execute(command, &params).unwrap();
    }
    assert!(s.model.is_stale());
    let document = s.doc.clone();
    let model = s.model.state();
    let before = (s.undo.len(), s.redo.len(), s.revision, s.evaluations);
    s.execute("render.lights.list", &json!({"settings":{"job":{"method":"cpu_raytrace"}}})).unwrap();
    assert!(s.model.is_stale());
    assert!(Arc::ptr_eq(&document, &s.doc));
    assert!(Arc::ptr_eq(&model, &s.model.state()));
    assert_eq!(before, (s.undo.len(), s.redo.len(), s.revision, s.evaluations));
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
    let world = s.world_state();
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
    assert!(Arc::ptr_eq(&world, &s.world_state()));
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

#[test]
fn render_lights_shape_falloff_and_world_vectors_lower_to_existing_fields_with_undo() {
    use crate::schema::render_lights::Geometry;
    let mut s = Session::default();
    for method in ["cpu_raytrace", "gpu_path"] {
        light_fixture(&mut s, method);
        s.execute("render.lights.add", &json!({"name":"Studio","set":{"kind":"area"}})).unwrap();
        let before = s.doc.clone();
        s.execute(
            "render.lights.set",
            &json!({"name":"Studio","id":"area:0","set":{
                "position_mm":[2,3,40],"width_mm":60,"height_mm":40,"radiance":[8,4,2]
            }}),
        )
        .unwrap();
        let lights = crate::render_lights::inspect(&s.doc.render.setups["Studio"]).unwrap();
        assert_eq!(lights[0].colour.map(|c| c * lights[0].intensity), [8.0, 4.0, 2.0]);
        let Geometry::Area { position_mm, edge_u_mm: [ux, uy, uz], edge_v_mm: [vx, vy, vz] } = lights[0].geometry else { panic!("area") };
        assert_eq!(position_mm, [2.0, 3.0, 40.0]);
        assert!((ux.hypot(uy).hypot(uz) - 60.0).abs() < 1e-9);
        assert!((vx.hypot(vy).hypot(vz) - 40.0).abs() < 1e-9);
        let changed = s.doc.clone();
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc, before);
        s.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(s.doc, changed);
        // GPU stores half-edges, CPU full sides. Both inspect as the same full vectors.
        s.execute("render.lights.set", &json!({"name":"Studio","id":"area:0","set":{"edge_u_mm":[0,60,0],"edge_v_mm":[40,0,0]}})).unwrap();
        let roundtrip = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
        assert_eq!(roundtrip, *s.doc);
        s.execute("render.lights.add", &json!({"name":"Studio","set":{"kind":"directional"}})).unwrap();
        s.execute("render.lights.set", &json!({"name":"Studio","id":"directional:0","set":{"direction":[-0.4,-0.6,0.7]}})).unwrap();
        let lights = crate::render_lights::inspect(&s.doc.render.setups["Studio"]).unwrap();
        let Geometry::Directional { direction, .. } = lights[0].geometry else { panic!("directional") };
        assert_eq!(direction, [-0.4, -0.6, 0.7]);
    }
    light_fixture(&mut s, "cpu_raytrace");
    s.execute("render.lights.add", &json!({"name":"Studio","set":{"kind":"point"}})).unwrap();
    s.execute(
        "render.lights.set",
        &json!({"name":"Studio","id":"point:0","set":{
            "position_mm":[5,10,20],"reference_distance_mm":1000,"minimum_distance_mm":10
        }}),
    )
    .unwrap();
    let lights = crate::render_lights::inspect(&s.doc.render.setups["Studio"]).unwrap();
    assert!(matches!(
        lights[0].geometry,
        Geometry::Point { position_mm: [5.0, 10.0, 20.0], reference_distance_mm: 1000.0, minimum_distance_mm: 10.0 }
    ));
}

#[test]
fn render_lights_new_numeric_fields_refuse_hostile_and_inapplicable_patches_atomically() {
    use crate::schema::render_lights::LightPatch;
    let mut s = Session::default();
    light_fixture(&mut s, "cpu_raytrace");
    for kind in ["area", "point", "directional"] {
        s.execute("render.lights.add", &json!({"name":"Studio","set":{"kind":kind}})).unwrap();
    }
    for (id, set) in [
        ("area:0", json!({"width_mm":1e300})),
        ("area:0", json!({"height_mm":-1})),
        ("area:0", json!({"radiance":[1e300,0,0]})),
        ("area:0", json!({"edge_u_mm":[0,0,0]})),
        ("area:0", json!({"edge_v_mm":[1,0,0],"edge_u_mm":[1,0,0]})),
        ("area:0", json!({"position_mm":[1e300,0,0]})),
        ("area:0", json!({"minimum_distance_mm":1})),
        ("point:0", json!({"width_mm":1})),
        ("point:0", json!({"reference_distance_mm":1e300})),
        ("point:0", json!({"reference_distance_mm":1,"minimum_distance_mm":2})),
        ("point:0", json!({"minimum_distance_mm":0})),
        ("directional:0", json!({"direction":[0,0,0]})),
        ("directional:0", json!({"direction":[1e300,0,0]})),
    ] {
        let before = s.doc.clone();
        let history = (s.undo.len(), s.revision);
        assert!(s.execute("render.lights.set", &json!({"name":"Studio","id":id,"set":set})).is_err());
        assert!(Arc::ptr_eq(&before, &s.doc));
        assert_eq!((s.undo.len(), s.revision), history);
    }
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for edit in [
            LightPatch { width_mm: Some(invalid), ..Default::default() },
            LightPatch { height_mm: Some(invalid), ..Default::default() },
            LightPatch { radiance: Some([invalid, 1.0, 1.0]), ..Default::default() },
            LightPatch { direction: Some([invalid, 0.0, 1.0]), ..Default::default() },
            LightPatch { position_mm: Some([invalid, 0.0, 1.0]), ..Default::default() },
            LightPatch { reference_distance_mm: Some(invalid), ..Default::default() },
            LightPatch { minimum_distance_mm: Some(invalid), ..Default::default() },
        ] {
            let mut light = crate::render_lights::inspect(&s.doc.render.setups["Studio"]).unwrap().remove(0);
            assert!(crate::render_lights::patch(&mut light, &edit, [0.0; 3], 100.0).is_err());
        }
    }
}

#[test]
fn render_lights_removing_last_area_clears_area_only_sampling_settings() {
    let mut s = Session::default();
    light_fixture(&mut s, "cpu_raytrace");
    s.execute("render.lights.add", &json!({"name":"Studio","set":{"kind":"area"}})).unwrap();
    let mut setup = s.doc.render.setups["Studio"].clone();
    if let Some(RenderJob::CpuRaytrace { options }) = &mut setup.job {
        options.area_samples = Some(64);
        options.seed = Some(123);
    }
    s.execute("render.lights.remove", &json!({"name":"Studio","id":"area:0","settings":setup})).unwrap();
    let Some(RenderJob::CpuRaytrace { options }) = &s.doc.render.setups["Studio"].job else { panic!("CPU") };
    assert!(options.area_samples.is_none() && options.seed.is_none());
    assert!(options.area_lights.as_ref().unwrap().is_empty());
}
