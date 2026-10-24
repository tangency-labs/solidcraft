//! Read-only discovery for the original effect editors; no device or capture.
use crate::{Result, Session, params::bad};
use serde_json::{Value, json};
use solidcraft_render::job::{
    aging::{AgingKind, AgingLayer},
    looks::{LookKind, LookLayer},
};
fn empty(p: &Value, command: &str) -> Result<()> {
    if !p.as_object().is_some_and(|value| value.is_empty()) {
        return Err(bad(command, "this discovery command accepts an empty object only"));
    }
    Ok(())
}
pub(super) fn aging(_: &mut Session, p: &Value) -> Result<Value> {
    empty(p, "render.aging.describe")?;
    Ok(json!({"maximum_layers":8,"storage":"GPU saved job.options.aging","order":"bottom to top; later layers modify earlier surface results",
        "presets":AgingKind::ALL.map(AgingLayer::preset),"target":"all eligible opaque bodies or stable definition/feature/component/occurrence identity; capture indices are resolved only for the runtime job; passive stages and glass are excluded",
        "coordinate_space":"world millimetres, explicit origin and 4096-cell periodic field; not object-following motion",
        "preserves":["engineering geometry","mass and physical material cards","pose","source textures","visibility"],
        "limitations":["authored optical weathering, not measured corrosion or age prediction","no geometric grooves or edge-curvature detection","no aging on glass or participating-medium boundaries","stack changes transport and requires a new render"],
        "airborne":"use the separately validated finite homogeneous GPU medium; it is not deposited surface dust or a physical lunar atmosphere"}))
}
pub(super) fn presentation(_: &mut Session, p: &Value) -> Result<Value> {
    empty(p, "render.effects.describe")?;
    Ok(json!({"maximum_look_layers":8,"storage":"GPU saved job.options.finishing; retained-master presentation uses the same schema",
        "looks":LookKind::ALL.map(|kind|LookLayer{kind,..Default::default()}),
        "photographic_default":solidcraft_render::job::photographic::Photographic::default(),
        "flare_source":"up to four separated scene-linear high-luminance samples from the actual image; no arbitrary light source is invented",
        "preserves":["raw FLOAT32 EXR bytes","captured geometry and lighting","engineering materials"],
        "limitations":["original artistic transforms, not film-brand LUTs or calibrated camera emulation","image-space flare/distortion/chromatic shifts are not physical lens transport","spatial photographic effects require opaque output and use clamped edges","regrading does not rerun denoising or trace rays"]}))
}
pub(super) fn resolve_aging(
    session: &Session,
    scene: &solidcraft_render::ProductScene,
    saved: &solidcraft_doc::render_aging::SavedAgingStack,
    command: &str,
) -> Result<solidcraft_render::job::aging::AgingStack> {
    use solidcraft_doc::DocError;
    use solidcraft_render::job::aging::{BodyName, BodyTarget};
    if saved.is_empty() {
        return Ok(Default::default());
    }
    let identities = super::surface_bindings::targets(session, command)?;
    let visible: std::collections::BTreeMap<_, _> = scene.meshes.iter().enumerate().map(|(i, body)| (body.name.as_str(), i)).collect();
    saved
        .resolve(|target| {
            let name = identities
                .get(target)
                .ok_or_else(|| DocError::Invalid("saved aging target is missing or replaced; explicit rebinding required".into()))?;
            let index =
                visible.get(name.as_str()).ok_or_else(|| DocError::Invalid("saved aging target is hidden or absent from this render".into()))?;
            let body_index = u32::try_from(*index).map_err(|_| DocError::Invalid("aging capture index exceeds runtime range".into()))?;
            let body_name = BodyName::new(name).map_err(|e| DocError::Invalid(e.to_string()))?;
            Ok(BodyTarget { body_index, body_name })
        })
        .map_err(|e| bad(command, e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn effect_discovery_is_read_only_bounded_and_rejects_mutation_arguments() {
        let mut s = Session::default();
        let document = s.doc.to_json();
        let revision = s.revision;
        for command in ["render.aging.describe", "render.effects.describe"] {
            let reply = s.execute(command, &json!({})).unwrap();
            assert!(reply.is_object());
            assert!(s.execute(command, &json!({"apply":true})).is_err());
            assert_eq!(s.doc.to_json(), document);
            assert_eq!(s.revision, revision);
        }
        assert_eq!(s.execute("render.aging.describe", &json!({})).unwrap()["presets"].as_array().unwrap().len(), 7);
        assert_eq!(s.execute("render.effects.describe", &json!({})).unwrap()["looks"].as_array().unwrap().len(), 8);
    }
    #[test]
    fn saved_aging_tracks_its_occurrence_across_capture_reordering_and_cold_reopen() {
        use solidcraft_doc::render_aging::SavedAgingStack;
        use solidcraft_render::job::aging::{AgingKind, AgingLayer};
        let mut session = Session::default();
        let component = session.execute("component.create", &json!({"name":"Weathered part"})).unwrap();
        session.execute("solid.box", &json!({"body_name":"Panel","length":10,"width":10,"height":2})).unwrap();
        session.execute("component.activate", &json!({"component":0})).unwrap();
        session.execute("occurrence.copy", &json!({"component":component["component"],"translate":[30,0,0]})).unwrap();
        let identities = super::super::surface_bindings::targets(&session, "test").unwrap();
        let (target, name) = identities.iter().find(|(_, name)| name.as_str() == "Panel").unwrap();
        let target = target.clone();
        let name = name.clone();
        let mut layers = SavedAgingStack::default();
        layers.push(AgingLayer::preset(AgingKind::Dust)).unwrap();
        layers.get_mut(0).unwrap().target = Some(target.clone());
        let physical = session.execute("component.properties", &json!({})).unwrap();
        session.execute("render.settings.set", &json!({"name":"Aging","set":{"job":{"method":"gpu_path","options":{"aging":layers}}}})).unwrap();
        assert_eq!(session.execute("component.properties", &json!({})).unwrap(), physical);
        let original = crate::product_scene::extract(&session).unwrap();
        let first = resolve_aging(&session, &original, &layers, "test").unwrap().get(0).unwrap().target.unwrap();
        let mut reordered = original.clone();
        reordered.meshes.reverse();
        let moved = resolve_aging(&session, &reordered, &layers, "test").unwrap().get(0).unwrap().target.unwrap();
        assert_ne!(moved.body_index, first.body_index);
        assert_eq!(moved.body_name, first.body_name);
        assert_eq!(reordered.meshes[moved.body_index as usize].name, name);
        let native = solidcraft_io::write_design_container(&session.doc).unwrap();
        let json = session.doc.to_json();
        let native_doc = solidcraft_io::read_design(&native).unwrap();
        assert_eq!(native_doc.render, session.doc.render);
        assert_eq!(solidcraft_doc::Document::from_json(&json).unwrap().render, session.doc.render);
        session.execute("edit.undo", &json!({})).unwrap();
        assert!(!session.doc.render.setups.contains_key("Aging"));
        session.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(session.doc.render, native_doc.render);
        // Upstream grounds the first occurrence. Keep that refusal explicit before
        // exercising a deliberately movable instance and its stable render binding.
        let grounded_document = session.doc.clone();
        let grounded_history = (session.revision, session.undo.len(), session.redo.len());
        let movement = json!({"occurrence":component["occurrence"],"translate":[12,0,0]});
        let error = session.execute("occurrence.move", &movement).unwrap_err().to_string();
        assert!(error.contains("is grounded"), "{error}");
        assert_eq!(session.doc, grounded_document);
        assert_eq!((session.revision, session.undo.len(), session.redo.len()), grounded_history);
        session.execute("occurrence.ground", &json!({"occurrence":component["occurrence"],"grounded":false})).unwrap();
        session.execute("occurrence.move", &movement).unwrap();
        let changed = crate::product_scene::extract(&session).unwrap();
        let resolved = resolve_aging(&session, &changed, &layers, "test").unwrap();
        assert_eq!(resolved.get(0).unwrap().target.unwrap().body_name.as_str(), name);
        let mut wrong = layers.clone();
        wrong.get_mut(0).unwrap().target.as_mut().unwrap().feature += 100;
        assert!(resolve_aging(&session, &changed, &wrong, "test").unwrap_err().to_string().contains("rebind"));
        session.visibility.hidden_bodies.push(name);
        assert!(
            resolve_aging(&session, &crate::product_scene::extract(&session).unwrap(), &layers, "test").unwrap_err().to_string().contains("hidden")
        );
        assert_eq!(layers.get(0).unwrap().target.as_ref(), Some(&target));
    }

    #[test]
    fn legacy_ordinal_aging_is_refused_atomically_instead_of_retargeted() {
        let mut session = Session::default();
        session.execute("solid.box", &json!({"body_name":"Panel","length":2,"width":2,"height":2})).unwrap();
        let document = session.doc.clone();
        let history = (session.revision, session.undo.len());
        let request = json!({"name":"Legacy","set":{"job":{"method":"gpu_path","options":{"aging":[{"kind":"dust","target":{"body_index":0,"body_name":"Panel"}}]}}}});
        let error = session.execute("render.settings.set", &request).unwrap_err().to_string();
        assert!(error.contains("rebinding"), "{error}");
        assert_eq!(session.doc, document);
        assert_eq!((session.revision, session.undo.len()), history);
    }

    /// An executor identifier is not evidence of support for optical aging.
    /// Successful command-path execution lives with the real GPU executor tests.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn saved_targeted_aging_is_refused_by_an_executor_without_aging() {
        use solidcraft_render::job::{self, ImageExecutor, ImageHost};
        use std::sync::Arc;
        struct UnsupportedGpu;
        impl ImageExecutor for UnsupportedGpu {
            fn descriptor(&self) -> job::Result<solidcraft_render::ExecutorDescriptor> {
                let mut descriptor = job::CpuHost::cooperative().descriptor()?;
                descriptor.id = solidcraft_render::ExecutorId::new("gpu-path").unwrap();
                descriptor.profiles[0].features.push(solidcraft_render::RenderFeature::Progressive);
                Ok(descriptor)
            }
            fn admit(&self, _: &solidcraft_render::ProductScene, _: &job::ImageRequest, _: job::JobOptions) -> job::Result<job::Admission> {
                panic!("unsupported aging must be refused before executor admission")
            }
            fn begin(
                &self,
                _: solidcraft_render::ProductScene,
                _: job::ImageRequest,
                _: job::JobOptions,
                _: job::CancelToken,
            ) -> job::Result<Box<dyn job::ImageWork>> {
                panic!("unsupported aging must never start work")
            }
        }
        let mut session = Session::default();
        session.execute("solid.box", &json!({"body_name":"Target panel","length":2,"width":2,"height":1})).unwrap();
        let identities = super::super::surface_bindings::targets(&session, "test").unwrap();
        let target = identities.keys().next().unwrap().clone();
        let mut layers = solidcraft_doc::render_aging::SavedAgingStack::default();
        layers.push(solidcraft_render::job::aging::AgingLayer::default()).unwrap();
        layers.get_mut(0).unwrap().target = Some(target);
        session
            .execute(
                "render.settings.set",
                &json!({"name":"Aging","set":{"width":4,"height":4,"job":{"method":"gpu_path","options":{"samples":1,"aging":layers}}}}),
            )
            .unwrap();
        session = Session::new(solidcraft_io::read_design(&solidcraft_io::write_design_container(&session.doc).unwrap()).unwrap());
        session.set_render_job_host(Some(job::CpuHost::cooperative())).unwrap();
        let host = ImageHost::with_executor(Arc::new(UnsupportedGpu)).unwrap();
        session.register_render_job_host(host.clone()).unwrap();
        let doc = session.doc.clone();
        let before = (session.revision, session.undo.len(), session.redo.len());
        let saved = session.doc.render.setups["Aging"].clone();
        for command in ["render.preflight", "render.start"] {
            for request in [json!({"setup":"Aging"}), json!({"settings":saved})] {
                let error = session.execute(command, &request).unwrap_err().to_string();
                assert!(error.contains("this executor cannot evaluate ordered surface aging"), "{error}");
                assert_eq!(host.admitted_bytes(), 0);
            }
        }
        assert!(Arc::ptr_eq(&doc, &session.doc));
        assert_eq!(before, (session.revision, session.undo.len(), session.redo.len()));
        session.visibility.hidden_bodies.push("Target panel".into());
        assert!(session.execute("render.preflight", &json!({"setup":"Aging"})).unwrap_err().to_string().contains("hidden"));
    }
}
