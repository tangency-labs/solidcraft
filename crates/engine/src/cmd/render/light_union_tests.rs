use crate::Session;
use serde_json::{Value, json};
#[cfg(all(feature = "raytrace-cpu", not(target_arch = "wasm32")))]
use solidcraft_render::job::RaytraceOptions;
use solidcraft_render::job::{CpuHost, DirectionalLightOptions, PunctualLightOptions, RectangleLightOptions};

fn params() -> Value {
    let area = RectangleLightOptions {
        center_mm: [0.0, 0.0, 10.0],
        edge_u_mm: [8.0, 0.0, 0.0],
        edge_v_mm: [0.0, -0.5, 0.0],
        color: [1.0; 3],
        intensity: 2.0,
    };
    let point = PunctualLightOptions {
        position_mm: [0.0, 0.0, 10.0],
        color: [1.0; 3],
        intensity: 1.0,
        reference_distance_mm: 10.0,
        minimum_distance_mm: 0.001,
        cone: None,
    };
    let directional = DirectionalLightOptions { direction: [0.0, 0.0, 1.0], color: [1.0; 3], intensity: 1.0 };
    json!({"integrator":"raytrace","settings":{"width":8,"height":8},"raytrace":{"primary_samples":16,"lights":vec![directional;8],"punctual_lights":vec![point;8],"area_lights":vec![area;3],"area_samples":64},"led_strip":{"center_mm":[0,0,10],"length_mm":8,"width_mm":0.5,"length_axis":[1,0,0],"width_axis":[0,-1,0],"color":[1,0.5,0.25],"intensity":2},"led_samples":64})
}

#[cfg(all(feature = "raytrace-cpu", not(target_arch = "wasm32")))]
#[test]
fn normalized_twenty_source_preflight_has_one_metadata_authority_and_no_reservation() {
    let host = CpuHost::for_platform().unwrap();
    let mut s = Session::default();
    s.set_render_job_host(Some(host.clone())).unwrap();
    let doc = s.doc.clone();
    let before = (s.revision, s.evaluations, s.undo.len(), s.redo.len());
    let input = params();
    let unchanged = input.clone();
    let reply = s.execute("render.preflight", &input).unwrap();
    let options: RaytraceOptions = serde_json::from_value(reply["request"]["raytrace"].clone()).unwrap();
    assert_eq!(options.primary_samples, 16);
    assert_eq!(options.area_lights.as_ref().unwrap().len(), 4);
    assert_eq!(options.punctual_lights.as_ref().unwrap().len(), 8);
    assert_eq!(
        reply["retained_request_bytes"],
        json!(
            options.retained_bytes().unwrap()
                + serde_json::from_value::<solidcraft_render::job::ImageRequest>(reply["request"].clone())
                    .unwrap()
                    .retained_resource_metadata_bytes()
        )
    );
    assert_eq!(host.admitted_bytes(), 0);
    assert_eq!(input, unchanged);
    assert!(std::sync::Arc::ptr_eq(&doc, &s.doc));
    assert_eq!(before, (s.revision, s.evaluations, s.undo.len(), s.redo.len()));
}

#[test]
fn invalid_or_unavailable_union_preserves_stale_scene_before_capture_or_replacement() {
    let host = CpuHost::for_platform().unwrap();
    let mut s = Session::default();
    s.set_render_job_host(Some(host.clone())).unwrap();
    for (command, input) in [
        ("sketch.create", json!({"plane":"XY"})),
        ("sketch.rectangle.two_point", json!({"p0":[5,5],"p1":[25,20]})),
        ("sketch.finish", json!({})),
        ("solid.extrude", json!({"distance":5})),
        ("sketch.edit", json!({"sketch":"Sketch1"})),
        ("sketch.drag", json!({"entity":"l1.start","from":[5,5],"to":[4,5],"live":true})),
    ] {
        s.execute(command, &input).unwrap();
    }
    assert!(s.model.is_stale());
    let doc = s.doc.clone();
    let before = (s.revision, s.evaluations, s.undo.len(), s.redo.len());
    for command in ["render.preflight", "render.start"] {
        let mut input = params();
        input["replace"] = json!(true);
        input["led_samples"] = json!(4);
        let error = s.execute(command, &input).unwrap_err().to_string();
        assert!(error.contains("area_samples") || error.contains("raytrac"), "{error}");
        assert!(s.model.is_stale());
        assert!(std::sync::Arc::ptr_eq(&doc, &s.doc));
        assert_eq!(before, (s.revision, s.evaluations, s.undo.len(), s.redo.len()));
        assert_eq!(host.admitted_bytes(), 0);
    }
}

#[test]
fn cpu_finite_sun_common_edit_keeps_raw_radiance_identity_and_undo() {
    let mut s = Session::default();
    let sun = json!({"direction":[0.2,-0.3,0.9],"angular_radius_rad":0.02,"radiance":[301.25,202.5,103.75]});
    s.execute("render.settings.set",&json!({"name":"CPU Sun","set":{"width":16,"height":12,"job":{"method":"cpu_raytrace","options":{
        "path":{"max_depth":4},"progressive":{"samples_per_pixel":3,"seed":81},"sun":sun,"punctual_lights":[],"area_lights":[],"lights":[{"direction":[0,0,1],"color":[1,0.5,0.25],"intensity":2}]}}}})).unwrap();
    let before = s.doc.render.setups["CPU Sun"].clone();
    let rows = crate::render_lights::inspect(&before).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].id, "directional:0");
    assert_eq!(rows[1].id, "directional:1");
    let mut exact = before.clone();
    crate::render_lights::write(&mut exact, &rows).unwrap();
    assert_eq!(exact, before);
    let undo = s.undo.len();
    s.execute("render.lights.set", &json!({"name":"CPU Sun","id":"directional:1","set":{"intensity_ev":2.0}})).unwrap();
    let changed = s.doc.render.setups["CPU Sun"].clone();
    assert_eq!(serde_json::to_value(&changed).unwrap()["job"]["options"]["sun"], sun);
    let edited = crate::render_lights::inspect(&changed).unwrap();
    assert_eq!((edited[1].intensity, edited[1].colour), (4.0, [1.0, 0.5, 0.25]));
    assert_eq!(edited[1].geometry, rows[1].geometry);
    assert_eq!(s.undo.len(), undo + 1);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc.render.setups["CPU Sun"], before);
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(s.doc.render.setups["CPU Sun"], changed);
    let document = s.doc.clone();
    let history = (s.undo.len(), s.redo.len(), s.revision);
    let mut without_path = serde_json::to_value(&changed).unwrap();
    without_path["job"]["options"]["path"] = Value::Null;
    assert!(s.execute("render.settings.set", &json!({"name":"CPU Sun","set":without_path})).is_err());
    assert!(std::sync::Arc::ptr_eq(&document, &s.doc));
    assert_eq!(history, (s.undo.len(), s.redo.len(), s.revision));
    // Radius zero retains the pre-existing hard-source representation without Path.
    s.execute("render.settings.set",&json!({"name":"CPU hard","set":{"job":{"method":"cpu_raytrace","options":{"lights":[{"direction":[0,0,1],"color":[1,1,1],"intensity":1}]}}}})).unwrap();
    let hard = crate::render_lights::inspect(&s.doc.render.setups["CPU hard"]).unwrap();
    assert!(matches!(hard[0].geometry, crate::schema::render_lights::Geometry::Directional { angular_radius_rad: None, .. }));
}

#[test]
fn cpu_sun_add_remove_returns_actual_canonical_addresses_and_preserves_other_sources() {
    let mut s = Session::default();
    s.execute("render.settings.set",&json!({"name":"CPU rig","set":{"width":16,"height":12,"job":{"method":"cpu_raytrace","options":{
        "path":{"max_depth":4},"progressive":{"samples_per_pixel":3,"seed":81},"punctual_lights":[],"area_lights":[],"lights":[{"direction":[0,0,1],"color":[1,0.5,0.25],"intensity":2}]}}}})).unwrap();
    let before = s.doc.render.setups["CPU rig"].clone();
    let reply = s.execute("render.lights.add", &json!({"name":"CPU rig","set":{"kind":"directional","softness":0.1}})).unwrap();
    assert_eq!(reply["id"], "directional:0");
    let rows = crate::render_lights::inspect(&s.doc.render.setups["CPU rig"]).unwrap();
    assert_eq!(rows[1].id, "directional:1");
    assert_eq!(reply["id_remap"][0]["after_id"], "directional:1");
    let mut filtered = s.doc.render.setups["CPU rig"].clone();
    let visibility: crate::schema::render_lights::Visibility = serde_json::from_value(json!({"muted":["directional:0"]})).unwrap();
    crate::render_lights::filter(&mut filtered, &visibility).unwrap();
    let Some(solidcraft_doc::render_settings::RenderJob::CpuRaytrace { options, .. }) = filtered.job else { panic!("CPU expected") };
    assert_eq!(options.sun.unwrap().radiance, [0.0; 3]);
    assert_eq!(options.lights.as_ref().unwrap()[0].intensity, 2.0);
    s.execute("render.lights.remove", &json!({"name":"CPU rig","id":"directional:0"})).unwrap();
    assert_eq!(s.doc.render.setups["CPU rig"], before);
}
