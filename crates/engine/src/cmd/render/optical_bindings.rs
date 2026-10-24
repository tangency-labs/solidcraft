//! Stable, render-only optical assignments. Raw request overrides remain a
//! separate mode; saved optics cannot silently disappear into another target.
use crate::{Result, Session, params::bad};
use serde::Deserialize;
use serde_json::{Value, json};
use solidcraft_doc::{
    render_optics::{OpticalBindings, OpticalMaterial, SavedOptics},
    render_settings::RenderJob,
    render_surface::RenderBodyTarget,
};
use solidcraft_render::ProductScene;
use std::{collections::BTreeMap, sync::Arc};

pub(super) fn resolve(s: &Session, scene: &ProductScene, bindings: &OpticalBindings, command: &str) -> Result<Vec<super::transmission::Override>> {
    bindings.validate().map_err(|e| bad(command, e.to_string()))?;
    let identities = super::surface_bindings::targets(s, command)?;
    let visible: BTreeMap<_, _> = scene.meshes.iter().enumerate().map(|(i, b)| (b.name.as_str(), i)).collect();
    let mut result = Vec::new();
    result.try_reserve_exact(bindings.len()).map_err(|_| bad(command, "optical request allocation failed"))?;
    for binding in bindings.iter() {
        let name = identities.get(&binding.target).ok_or_else(|| bad(command, "saved optical target changed; explicit rebinding required"))?;
        let index = visible.get(name.as_str()).ok_or_else(|| bad(command, "saved optical target is hidden or absent"))?;
        let mut value = serde_json::to_value(binding.optics).map_err(|e| bad(command, e.to_string()))?;
        value["body_index"] = json!(index);
        value["body_name"] = json!(name);
        result.push(serde_json::from_value(value).map_err(|e| bad(command, e.to_string()))?);
    }
    Ok(result)
}
fn empty() -> Value {
    json!({})
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Bind {
    name: String,
    #[serde(default = "empty")]
    set: Value,
    body_index: usize,
    body_name: String,
    optics: OpticalMaterial,
    expected_revision: u64,
    #[serde(default)]
    replace: bool,
}
pub(super) fn bind(s: &mut Session, params: &Value) -> Result<Value> {
    let command = "render.optics.bind";
    let input: Bind = serde_json::from_value(params.clone()).map_err(|e| bad(command, e.to_string()))?;
    input.optics.validate().map_err(|e| bad(command, e.to_string()))?;
    if input.expected_revision != s.revision {
        return Err(bad(command, "optical binding review is stale"));
    }
    let mut setup = super::patched_settings(s, &input.name, &input.set, command)?;
    if s.doc.render.setups.get(&input.name).is_some_and(|old| old.optics != setup.optics) {
        return Err(bad(command, "setup patch cannot erase or replace saved optics; use explicit bind/unbind"));
    }
    if !matches!(setup.job, Some(RenderJob::GpuPath { .. })) {
        return Err(bad(command, "portable optics require an explicit GPU setup"));
    }
    let mut scene = crate::product_scene::extract(s)?;
    let name = &scene
        .meshes
        .get(input.body_index)
        .filter(|b| b.name == input.body_name)
        .ok_or_else(|| bad(command, "optical target ordinal and exact name do not match"))?
        .name;
    let identities = super::surface_bindings::targets(s, command)?;
    let mut matches = identities.iter().filter(|(_, world)| *world == name);
    let target = matches.next().map(|(t, _)| t.clone()).ok_or_else(|| bad(command, "optical target has no stable source identity"))?;
    if matches.next().is_some() {
        return Err(bad(command, "ambiguous optical target"));
    }
    setup.optics.set(SavedOptics { target, optics: input.optics }, input.replace).map_err(|e| bad(command, e.to_string()))?;
    setup.validate().map_err(|e| bad(command, e.to_string()))?;
    let resolved = resolve(s, &scene, &setup.optics, command)?;
    super::transmission::apply(&mut scene, &resolved, command)?;
    let mut settings = (*s.doc.render).clone();
    settings.setups.insert(input.name.clone(), setup.clone());
    settings.validate().map_err(|e| bad(command, e.to_string()))?;
    if settings != *s.doc.render {
        s.doc_mut().render = Arc::new(settings);
    }
    Ok(
        json!({"name":input.name,"setup":setup,"geometry_and_engineering_unchanged":true,"certification":"closed topology and nested containment are checked by the selected image producer before transport"}),
    )
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Unbind {
    name: String,
    target: RenderBodyTarget,
    expected_revision: u64,
}
pub(super) fn unbind(s: &mut Session, params: &Value) -> Result<Value> {
    let command = "render.optics.unbind";
    let input: Unbind = serde_json::from_value(params.clone()).map_err(|e| bad(command, e.to_string()))?;
    input.target.validate().map_err(|e| bad(command, e.to_string()))?;
    if input.expected_revision != s.revision {
        return Err(bad(command, "optical removal review is stale"));
    }
    let mut settings = (*s.doc.render).clone();
    let setup = settings.setups.get_mut(&input.name).ok_or_else(|| bad(command, "unknown optical setup"))?;
    let removed = setup.optics.remove(&input.target);
    settings.validate().map_err(|e| bad(command, e.to_string()))?;
    if settings != *s.doc.render {
        s.doc_mut().render = Arc::new(settings);
    }
    Ok(json!({"name":input.name,"removed":removed}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn optics_are_stable_undoable_offline_and_never_guess_a_replaced_body() {
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length":10,"width":10,"height":4,"body_name":"Diffuser"})).unwrap();
        s.execute("render.settings.set", &json!({"name":"Studio","set":{"job":{"method":"gpu_path"}}})).unwrap();
        let before = s.doc.to_json();
        let geometry = s.doc.features.clone();
        let physical = s.doc.materials.clone();
        let body = json!({"name":"Studio","body_index":0,"body_name":"Diffuser","expected_revision":s.revision,
            "optics":{"ior":1.4,"subsurface":{"scattering_per_mm":0.4,"anisotropy":0.2}}});
        s.execute("render.optics.bind", &body).unwrap();
        let saved = s.doc.to_json();
        assert_eq!(s.doc.features, geometry);
        assert_eq!(s.doc.materials, physical);
        let reopened = solidcraft_doc::Document::from_json(&saved).unwrap();
        assert_eq!(reopened.render, s.doc.render);
        let scene = crate::product_scene::extract(&s).unwrap();
        assert_eq!(resolve(&s, &scene, &s.doc.render.setups["Studio"].optics, "check").unwrap().len(), 1);
        assert!(s.execute("render.optics.bind", &body).is_err(), "review revision is stale");
        assert_eq!(s.doc.to_json(), saved);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc.to_json(), before);
        s.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(s.doc.to_json(), saved);
        let mut changed = body;
        changed["expected_revision"] = json!(s.revision);
        changed["optics"]["ior"] = json!(1.6);
        assert!(s.execute("render.optics.bind", &changed).is_err());
        assert_eq!(s.doc.to_json(), saved);
        changed["replace"] = json!(true);
        s.execute("render.optics.bind", &changed).unwrap();
        let target = s.doc.render.setups["Studio"].optics.iter().next().unwrap().target.clone();
        s.execute("render.optics.unbind", &json!({"name":"Studio","target":target,"expected_revision":s.revision})).unwrap();
        assert!(s.doc.render.setups["Studio"].optics.is_empty());
    }
}
