//! Request-only coat overrides on immutable captured geometry; no saved appearance changes.
use crate::{Result, params::bad};
use serde::Deserialize;
use solidcore_record::Vec3;
use solidcraft_render::ProductScene;
use solidcraft_viewport::{
    clear_coat::ClearCoat,
    surface::{OpaqueSurface, srgb8_to_linear},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Override {
    body_index: usize,
    body_name: String,
    weight: f64,
    roughness: f64,
    ior: f64,
}
pub(super) fn apply(scene: &mut ProductScene, overrides: &[Override], command: &str) -> Result<()> {
    if overrides.len() > solidcraft_doc::render_coat::MAX_COAT_BINDINGS {
        return Err(bad(command, "at most 64 runtime clear-coat overrides are supported"));
    }
    let mut ordinals = std::collections::BTreeSet::new();
    for input in overrides {
        if input.body_name.is_empty()
            || input.body_name.len() > 1024
            || input.body_name.chars().any(char::is_control)
            || !ordinals.insert(input.body_index)
        {
            return Err(bad(command, "clear coats require bounded names and distinct captured body ordinals"));
        }
        let coat = ClearCoat::new(input.weight, input.roughness, input.ior).map_err(|e| bad(command, e.to_string()))?;
        apply_one(scene, input.body_index, &input.body_name, coat, command)?;
    }
    Ok(())
}

pub(super) fn apply_one(scene: &mut ProductScene, body_index: usize, body_name: &str, coat: ClearCoat, command: &str) -> Result<()> {
    let body = scene
        .meshes
        .get_mut(body_index)
        .filter(|m| m.name == body_name)
        .ok_or_else(|| bad(command, "clear-coat body ordinal and exact name must both match render.scene"))?;
    // A zero-weight override is a true no-op, including legacy appearances.
    if coat.weight() == 0.0 {
        return Ok(());
    }
    if body.transmission.is_some() || body.faces.iter().any(|f| f.transmission.is_some()) {
        return Err(bad(command, "clear coat and transmission cannot share a body"));
    }
    let surface = |color: [u8; 3], existing: Option<OpaqueSurface>| -> Result<OpaqueSurface> {
        let [r, g, b] = color.map(srgb8_to_linear);
        let base =
            existing.map(Ok).unwrap_or_else(|| OpaqueSurface::new(Vec3::new(r, g, b), 0.0, 0.5, 1.5)).map_err(|e| bad(command, e.to_string()))?;
        Ok(base.with_clear_coat(coat))
    };
    body.surface = Some(surface(body.color, body.surface)?);
    for face in &mut body.faces {
        face.surface = Some(surface(face.color, face.surface)?);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn coat_is_runtime_only_and_validates_exact_body_and_values() {
        let mut s = crate::Session::default();
        s.execute("solid.box", &json!({"length":10,"width":10,"height":2,"body_name":"Housing"})).unwrap();
        let saved = s.doc.to_json();
        let original = crate::product_scene::extract(&s).unwrap();
        let input = json!({"body_index":0,"body_name":"Housing","weight":1,"roughness":0.12,"ior":1.5});
        let mut scene = original.clone();
        apply(&mut scene, &[serde_json::from_value(input.clone()).unwrap()], "render.start").unwrap();
        assert!(scene.meshes[0].surface.unwrap().clear_coat().is_some());
        assert!(std::sync::Arc::ptr_eq(&scene.meshes[0].mesh, &original.meshes[0].mesh));
        assert_eq!(s.doc.to_json(), saved);
        assert!(crate::product_scene::extract(&s).unwrap().meshes[0].surface.is_none_or(|s| s.clear_coat().is_none()));
        for (key, value) in
            [("weight", json!(-0.1)), ("roughness", json!(0)), ("ior", json!(4)), ("body_index", json!(1)), ("body_name", json!("Wrong"))]
        {
            let mut bad_input = input.clone();
            bad_input[key] = value;
            assert!(apply(&mut original.clone(), &[serde_json::from_value(bad_input).unwrap()], "render.start").is_err());
        }
        let duplicate = serde_json::from_value::<Vec<Override>>(json!([input, input])).unwrap();
        assert!(apply(&mut original.clone(), &duplicate, "render.start").is_err());
    }
}

#[cfg(test)]
mod stable_tests {
    use super::*;
    use crate::Session;
    use serde_json::json;
    use solidcraft_doc::render_coat::{CoatBindings, CoatMaterial, SavedCoat};
    use std::sync::Arc;

    #[test]
    fn stable_coats_save_reopen_replace_and_undo_offline_for_cpu_and_gpu_without_changing_sources() {
        for method in ["cpu_raytrace", "gpu_path"] {
            let mut s = Session::default();
            s.execute("solid.box", &json!({"length":10,"width":10,"height":3,"body_name":"Housing"})).unwrap();
            s.execute("material.assign", &json!({"bodies":["Housing"],"material":"Steel"})).unwrap();
            s.execute("render.settings.set", &json!({"name":"Coat","set":{"width":8,"height":8,"job":{"method":method}}})).unwrap();
            let physical = s.execute("component.properties", &json!({})).unwrap();
            let model = s.model.state();
            let before = s.doc.clone();
            let undo = s.undo.len();
            let evaluations = s.evaluations;
            let coat = json!({"weight":0.63,"roughness":0.61,"ior":1.7});
            s.execute("render.coat.bind", &json!({"name":"Coat","body_index":0,"body_name":"Housing","coat":coat,"expected_revision":s.revision}))
                .unwrap();
            assert_eq!(s.undo.len(), undo + 1);
            assert_eq!(s.evaluations, evaluations);
            assert!(Arc::ptr_eq(&model, &s.model.state()));
            assert_eq!(s.doc.features, before.features);
            assert_eq!(s.doc.materials, before.materials);
            assert_eq!(physical, s.execute("component.properties", &json!({})).unwrap());
            let saved = s.doc.clone();
            let loaded = solidcraft_doc::Document::from_json(&saved.to_json()).unwrap();
            assert_eq!(loaded.render, saved.render);
            let original = crate::product_scene::extract(&s).unwrap();
            let mut scene = original.clone();
            let faces_capacity = scene.meshes[0].faces.capacity();
            let cpu = solidcraft_render::job::RaytraceOptions::default();
            super::super::coat_bindings::apply(
                &s,
                &mut scene,
                &saved.render.setups["Coat"].coats,
                (method == "cpu_raytrace").then_some(&cpu),
                "check",
            )
            .unwrap();
            let actual = scene.meshes[0].surface.unwrap().clear_coat().unwrap();
            assert_eq!((actual.weight(), actual.roughness(), actual.ior()), (0.63, 0.61, 1.7));
            assert!(Arc::ptr_eq(&original.meshes[0].mesh, &scene.meshes[0].mesh));
            assert_eq!(original.meshes[0].faces.len(), scene.meshes[0].faces.len());
            assert_eq!(faces_capacity, scene.meshes[0].faces.capacity());
            assert_eq!(s.doc, saved, "captured coat never changes the source appearance");
            for face in &scene.meshes[0].faces {
                assert_eq!(face.surface.unwrap().clear_coat(), Some(actual));
            }
            s.execute("edit.undo", &json!({})).unwrap();
            assert_eq!(s.doc, before);
            s.execute("edit.redo", &json!({})).unwrap();
            assert_eq!(s.doc, saved);
            let replacement = json!({"name":"Coat","body_index":0,"body_name":"Housing","coat":{"weight":0.4,"roughness":0.3,"ior":1.8},"expected_revision":s.revision});
            assert!(s.execute("render.coat.bind", &replacement).is_err());
            assert_eq!(s.doc, saved);
            let mut explicit = replacement;
            explicit["replace"] = json!(true);
            s.execute("render.coat.bind", &explicit).unwrap();
            assert_eq!(s.doc.render.setups["Coat"].coats.iter().next().unwrap().coat.ior, 1.8);
            s.execute("edit.undo", &json!({})).unwrap();
            assert_eq!(s.doc, saved);
            let target = saved.render.setups["Coat"].coats.iter().next().unwrap().target.clone();
            s.execute("render.coat.unbind", &json!({"name":"Coat","target":target,"expected_revision":s.revision})).unwrap();
            assert!(s.doc.render.setups["Coat"].coats.is_empty());
            s.execute("edit.undo", &json!({})).unwrap();
            assert_eq!(s.doc, saved);
        }
    }
    #[test]
    fn stable_coat_source_replacement_hidden_and_glass_guards_never_guess_or_mutate_a_binding() {
        for method in ["cpu_raytrace", "gpu_path"] {
            let mut s = Session::default();
            s.execute("solid.box", &json!({"length":4,"width":4,"height":2,"body_name":"Housing"})).unwrap();
            s.execute(
                "render.coat.bind",
                &json!({"name":"Coat","set":{"width":4,"height":4,"job":{"method":method}},
                "body_index":0,"body_name":"Housing","coat":{"weight":0},"expected_revision":s.revision}),
            )
            .unwrap();
            let saved = s.doc.render.setups["Coat"].coats.clone();
            let target = saved.iter().next().unwrap().target.clone();
            let original = crate::product_scene::extract(&s).unwrap();
            let mut zero = original.clone();
            super::super::coat_bindings::apply(&s, &mut zero, &saved, None, "check").unwrap();
            assert_eq!(zero.meshes[0].surface, original.meshes[0].surface);
            assert!(Arc::ptr_eq(&zero.meshes[0].mesh, &original.meshes[0].mesh));
            let before = s.doc.clone();
            assert!(
                s.execute(
                    "render.optics.bind",
                    &json!({"name":"Coat","body_index":0,"body_name":"Housing","optics":{},"expected_revision":s.revision})
                )
                .is_err()
            );
            assert_eq!(s.doc, before, "zero weight is authored opaque intent, not permission to silently drop it on Glass");
            let mut glass = original;
            glass.meshes[0].transmission = Some(solidcraft_render::transmission::Transmission::new(Vec3::ZERO, None).unwrap());
            assert!(super::super::coat_bindings::apply(&s, &mut glass, &saved, None, "check").unwrap_err().to_string().contains("opaque"));
            s.execute("browser.visibility", &json!({"items":["Housing"],"visible":false})).unwrap();
            let mut hidden = crate::product_scene::extract(&s).unwrap();
            assert!(super::super::coat_bindings::apply(&s, &mut hidden, &saved, None, "check").unwrap_err().to_string().contains("hidden"));
            s.execute("edit.undo", &json!({})).unwrap();
            s.execute("timeline.delete", &json!({"feature":target.feature.to_string()})).unwrap();
            s.execute("solid.box", &json!({"length":4,"width":4,"height":2,"body_name":"Housing"})).unwrap();
            let mut replaced = crate::product_scene::extract(&s).unwrap();
            assert_eq!((replaced.meshes.len(), replaced.meshes[0].name.as_str()), (1, "Housing"));
            assert!(super::super::coat_bindings::apply(&s, &mut replaced, &saved, None, "check").unwrap_err().to_string().contains("changed"));
            assert_eq!(s.doc.render.setups["Coat"].coats, saved);
            // Empty means the original capture path, even when no old target survives.
            super::super::coat_bindings::apply(&s, &mut replaced, &CoatBindings::default(), None, "check").unwrap();
        }
    }
    #[test]
    fn stable_cpu_coats_refuse_selected_transport_but_leave_authored_values_and_scene_sources_intact() {
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length":4,"width":4,"height":2,"body_name":"Housing"})).unwrap();
        let target = super::super::surface_bindings::targets(&s, "check").unwrap().into_keys().next().unwrap();
        let mut bindings = CoatBindings::default();
        bindings.set(SavedCoat { target, coat: CoatMaterial::default() }, false).unwrap();
        let scene = crate::product_scene::extract(&s).unwrap();
        let options = solidcraft_render::job::RaytraceOptions {
            reflections: Some(solidcraft_render::job::ReflectionOptions {
                samples: 1,
                max_bounces: 1,
                bodies: vec![solidcraft_render::job::ReflectionBody {
                    body: "Housing".into(),
                    mode: solidcraft_render::job::ReflectionMode::RoughSpecular,
                }],
            }),
            ..Default::default()
        };
        let before = s.doc.clone();
        let mut captured = scene.clone();
        let error = super::super::coat_bindings::apply(&s, &mut captured, &bindings, Some(&options), "check").unwrap_err().to_string();
        assert!(error.contains("selected reflection"));
        assert_eq!(captured.meshes[0].surface, scene.meshes[0].surface);
        assert_eq!(bindings.iter().next().unwrap().coat, CoatMaterial::default());
        assert_eq!(s.doc, before);
    }
}
