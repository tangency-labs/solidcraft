//! Capture source maps atomically, retain exact bytes, and bind a local physical frame.
use crate::schema::render_surface_resources::{
    ImportParams, ImportResult, ListParams, ListResult, RemoveParams, RemoveResult, ResourceInfo, TextureFile,
};
use crate::{Result, Session, params::bad, schema::TypedContract};
use serde_json::{Value, json};
use solidcraft_doc::{
    render_resources::HdrId,
    render_settings::RenderJob,
    render_surface::{EmbeddedSurfaceKind, EmbeddedSurfaceSource, SavedSurface, SurfaceSource},
    render_texture_resources::{MAX_TEXTURE_RESOURCE_BYTES, TextureProvenance, TextureResource, TextureResources},
};
use solidcraft_geom::MeshGeom;
use solidcraft_render::job::textures::PlanarFrame;
use solidcraft_render::texture_input::MapRole;
use std::sync::Arc;

pub(super) const IMPORT_CONTRACT: TypedContract<ImportParams> = TypedContract::new();
pub(super) const LIST_CONTRACT: TypedContract<ListParams> = TypedContract::new();
pub(super) const REMOVE_CONTRACT: TypedContract<RemoveParams> = TypedContract::new();

fn id(value: &str, command: &str) -> Result<HdrId> {
    serde_json::from_value(json!(value)).map_err(|e| bad(command, e.to_string()))
}
fn capture(
    input: Option<&TextureFile>,
    role: MapRole,
    resources: &mut TextureResources,
    provenance: &TextureProvenance,
    command: &str,
) -> Result<Option<HdrId>> {
    let Some(input) = input else { return Ok(None) };
    if input.path.is_empty() || input.path.len() > 4096 || input.path.chars().any(char::is_control) {
        return Err(bad(command, "texture import requires a bounded scoped local path"));
    }
    let expected = input.expected_sha256.as_deref().map(|value| id(value, command)).transpose()?;
    let bytes = solidcraft_io::vfs::read(&input.path, solidcraft_render::texture_input::MAX_TEXTURE_BYTES)
        .map_err(|e| bad(command, format!("could not capture texture: {e}")))?;
    if expected.is_some_and(|expected| expected != HdrId::of(&bytes)) {
        return Err(bad(command, "texture file changed since selection; inspect and select it again"));
    }
    // Validation and all resource inserts affect the private settings copy only.
    let resource = TextureResource::new(bytes, provenance.clone()).map_err(|e| bad(command, e.to_string()))?;
    resource.validate_role(role).map_err(|e| bad(command, e.to_string()))?;
    resources.insert(resource).map(Some).map_err(|e| bad(command, e.to_string()))
}

pub(super) fn import(s: &mut Session, params: &Value) -> Result<Value> {
    let command = "render.surface.import";
    let input: ImportParams = serde_json::from_value(params.clone()).map_err(|e| bad(command, e.to_string()))?;
    let mut setup = super::patched_settings(s, &input.name, &input.set, command)?;
    if !matches!(setup.job, Some(RenderJob::CpuRaytrace { .. } | RenderJob::GpuPath { .. })) {
        return Err(bad(command, "imported surface maps require an explicit CPU raytrace or GPU path setup"));
    }
    if s.doc.render.setups.get(&input.name).is_some_and(|previous| previous.surfaces != setup.surfaces) {
        return Err(bad(command, "setup patch cannot erase saved surface bindings; use explicit unbind"));
    }
    let frame = PlanarFrame { origin: input.frame.origin, u: input.frame.u, v: input.frame.v };
    frame.validate().map_err(|e| bad(command, e.to_string()))?;
    if !input.normal_strength.is_finite() || !(0.0..=1.0).contains(&input.normal_strength) {
        return Err(bad(command, "normal strength must be finite in 0..1"));
    }
    if input.maps.base_color_srgb.is_none()
        && input.maps.normal_opengl.is_none()
        && input.maps.roughness_linear.is_none()
        && input.maps.metallic_linear.is_none()
    {
        return Err(bad(command, "import at least one explicitly typed surface map"));
    }
    let scene = crate::product_scene::extract(s)?;
    let body = scene
        .meshes
        .get(input.body_index)
        .filter(|body| body.name == input.body_name)
        .ok_or_else(|| bad(command, "body ordinal and name must both match render.scene"))?;
    if body.opacity != 1.0 || body.transmission.is_some() || body.faces.iter().any(|face| face.opacity != 1.0 || face.transmission.is_some()) {
        return Err(bad(command, "imported maps support opaque bodies only"));
    }
    let bounds = body.mesh.bounds();
    for x in [bounds.min.x, bounds.max.x] {
        for y in [bounds.min.y, bounds.max.y] {
            for z in [bounds.min.z, bounds.max.z] {
                frame.uv(solidcore_record::Vec3::new(x, y, z)).map_err(|e| bad(command, e.to_string()))?;
            }
        }
    }
    let targets = super::surface_bindings::targets(s, command)?;
    let mut matching = targets.iter().filter(|(_, name)| **name == input.body_name);
    let target = matching.next().map(|(target, _)| target.clone()).ok_or_else(|| bad(command, "surface target has no stable identity"))?;
    if matching.next().is_some() {
        return Err(bad(command, "surface target has ambiguous identity"));
    }
    if !input.replace && setup.surfaces.iter().any(|binding| binding.target == target) {
        return Err(bad(command, "surface target already has a binding; explicit replacement required"));
    }
    let world = s.world_state()?;
    let computed = world.bodies.iter().find(|body| body.name == input.body_name).ok_or_else(|| bad(command, "surface world body disappeared"))?;
    let inverse = solidcraft_doc::mat_inverse(&computed.source_transform).ok_or_else(|| bad(command, "surface placement is not invertible"))?;
    let local_frame = super::surface_bindings::transform(frame, &inverse, command)?;
    let provenance = TextureProvenance { author: input.provenance.author, source: input.provenance.source, license: input.provenance.license };
    let mut settings = (*s.doc.render).clone();
    let source = EmbeddedSurfaceSource {
        kind: EmbeddedSurfaceKind::Embedded,
        base_color: capture(input.maps.base_color_srgb.as_ref(), MapRole::BaseColor, &mut settings.texture_resources, &provenance, command)?,
        normal_open_gl: capture(input.maps.normal_opengl.as_ref(), MapRole::NormalOpenGl, &mut settings.texture_resources, &provenance, command)?,
        roughness: capture(input.maps.roughness_linear.as_ref(), MapRole::Roughness, &mut settings.texture_resources, &provenance, command)?,
        metallic: capture(input.maps.metallic_linear.as_ref(), MapRole::Metallic, &mut settings.texture_resources, &provenance, command)?,
        flip_v: input.flip_v,
    };
    setup
        .surfaces
        .set(
            SavedSurface { target, source: SurfaceSource::Embedded(source.clone()), local_frame, normal_strength: input.normal_strength },
            input.replace,
        )
        .map_err(|e| bad(command, e.to_string()))?;
    settings.setups.insert(input.name.clone(), setup.clone());
    settings.validate().map_err(|e| bad(command, e.to_string()))?;
    if settings.texture_resources.total_bytes() > MAX_TEXTURE_RESOURCE_BYTES {
        return Err(bad(command, "encoded texture resources exceed the design budget"));
    }
    let resources = [source.base_color, source.normal_open_gl, source.roughness, source.metallic]
        .into_iter()
        .flatten()
        .map(|digest| digest.to_string())
        .collect();
    let result = ImportResult {
        name: input.name,
        setup: serde_json::to_value(setup).map_err(|e| bad(command, e.to_string()))?,
        resources,
        self_contained: true,
        mapping_space: "definition_local_mm".into(),
    };
    if settings != *s.doc.render {
        s.doc_mut().render = Arc::new(settings);
    }
    serde_json::to_value(result).map_err(|e| bad(command, e.to_string()))
}

fn users(s: &Session, id: HdrId) -> Vec<String> {
    s.doc
        .render
        .setups
        .iter()
        .filter(|(_, setup)| {
            setup.surfaces.iter().any(|binding| match &binding.source {
                SurfaceSource::Embedded(source) => [source.base_color, source.normal_open_gl, source.roughness, source.metallic].contains(&Some(id)),
                SurfaceSource::Builtin(_) => false,
            })
        })
        .map(|(name, _)| name.clone())
        .collect()
}
pub(super) fn list(s: &mut Session, params: &Value) -> Result<Value> {
    let command = "render.texture.list";
    let _: ListParams = serde_json::from_value(params.clone()).map_err(|e| bad(command, e.to_string()))?;
    let resources = s
        .doc
        .render
        .texture_resources
        .iter()
        .map(|(id, resource)| {
            Ok(ResourceInfo {
                sha256: id.to_string(),
                encoded_bytes: resource.bytes().len(),
                dimensions: resource.dimensions(),
                provenance: serde_json::to_value(resource.provenance()).map_err(|e| bad(command, e.to_string()))?,
                users: users(s, *id),
            })
        })
        .collect::<Result<_>>()?;
    let encoded_bytes = s.doc.render.texture_resources.iter().map(|(_, resource)| resource.bytes().len()).sum();
    serde_json::to_value(ListResult { resources, encoded_bytes }).map_err(|e| bad(command, e.to_string()))
}
pub(super) fn remove(s: &mut Session, params: &Value) -> Result<Value> {
    let command = "render.texture.remove";
    let input: RemoveParams = serde_json::from_value(params.clone()).map_err(|e| bad(command, e.to_string()))?;
    let id = id(&input.sha256, command)?;
    let users = users(s, id);
    if !users.is_empty() {
        return Err(bad(command, format!("texture is referenced by render setups: {}; unbind it first", users.join(", "))));
    }
    let mut settings = (*s.doc.render).clone();
    let removed = settings.texture_resources.remove(&id).is_some();
    settings.validate().map_err(|e| bad(command, e.to_string()))?;
    if removed {
        s.doc_mut().render = Arc::new(settings);
    }
    serde_json::to_value(RemoveResult { sha256: input.sha256, removed }).map_err(|e| bad(command, e.to_string()))
}
