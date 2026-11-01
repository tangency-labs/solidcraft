//! Shared authoritative metadata and payload representation for native containers and Git.
//! Storage-specific compression, caching and transport stay outside this module.
use super::*;

pub(crate) fn encode(doc: &Document, compression: DesignCompression, mut blob: impl FnMut(&[u8], &str, bool) -> Result<Value>) -> Result<Vec<u8>> {
    doc.render.validate().map_err(invalid)?;
    let mut features = Vec::with_capacity(doc.features.len());
    for feature in &doc.features {
        let replacements = match &feature.kind {
            FeatureKind::Import { step, .. } => {
                // STEP is text and shrinks several times: always compressed, so an import as large
                // as MAX_STEP_BYTES still fits in a design file.
                let blob = blob(step.as_bytes(), "step", true)?;
                fields([("step", Value::String(String::new())), ("step_blob", blob)])
            }
            FeatureKind::MeshImport { meshes, .. } => {
                let mut metadata = Vec::with_capacity(meshes.len());
                for mesh in meshes {
                    let bytes = encode_mesh(mesh)?;
                    // Fast saves store a mesh that fits. One past the file limit is deflated,
                    // or it could never be saved (#6600). Compact deflates every mesh.
                    let compress = compression == DesignCompression::Compact || bytes.len() > max_part();
                    let blob = blob(&bytes, "mesh", compress)?;
                    let mut value = replace(mesh, fields([("positions", json!([])), ("triangles", json!([]))])).map_err(invalid)?;
                    value.as_object_mut().ok_or_else(|| invalid("mesh metadata"))?.insert("mesh_blob".into(), blob);
                    metadata.push(value);
                }
                fields([("meshes", Value::Array(metadata))])
            }
            _ => Map::new(),
        };
        // Extra reference fields are added after serializing existing fields.
        let step_blob = replacements.get("step_blob").cloned();
        let mut value = replace(feature, replacements).map_err(invalid)?;
        if let Some(blob) = step_blob {
            value.as_object_mut().ok_or_else(|| invalid("feature metadata"))?.insert("step_blob".into(), blob);
        }
        features.push(value);
    }
    let mut canvases = Vec::with_capacity(doc.canvases.len());
    for canvas in &doc.canvases {
        if canvas.data.len() > solidcraft_doc::canvas::MAX_CANVAS_BYTES.div_ceil(3) * 4 {
            return Err(invalid("canvas too large"));
        }
        let bytes = canvas.bytes().ok_or_else(|| invalid("invalid canvas base64"))?;
        if !matches!(canvas.format.as_str(), "png" | "jpeg") || bytes.len() > solidcraft_doc::canvas::MAX_CANVAS_BYTES {
            return Err(invalid("invalid canvas format or size"));
        }
        let blob = blob(&bytes, &canvas.format, false)?;
        let mut value = replace(canvas, fields([("data", json!(""))])).map_err(invalid)?;
        value.as_object_mut().ok_or_else(|| invalid("canvas metadata"))?.insert("image_blob".into(), blob);
        canvases.push(value);
    }
    let mut clouds = Vec::with_capacity(doc.point_clouds.len());
    for cloud in &doc.point_clouds {
        cloud.check().map_err(invalid)?;
        let blob = blob(&encode_cloud(cloud)?, "points", compression == DesignCompression::Compact)?;
        let mut value = replace(cloud, fields([("points", json!("")), ("colors", json!(""))])).map_err(invalid)?;
        value.as_object_mut().ok_or_else(|| invalid("point cloud metadata"))?.insert("points_blob".into(), blob);
        clouds.push(value);
    }
    let mut replacements = fields([("features", Value::Array(features)), ("canvases", Value::Array(canvases))]);
    if !clouds.is_empty() {
        replacements.insert("point_clouds".into(), Value::Array(clouds));
    }
    if !doc.render.resources.is_empty() {
        let mut resources = Map::new();
        for (id, resource) in doc.render.resources.iter() {
            let blob = blob(resource.bytes(), "hdr", false)?;
            resources.insert(id.to_string(), json!({"kind":"radiance_hdr","data":"","hdr_blob":blob}));
        }
        let render = replace(doc.render.as_ref(), fields([("resources", Value::Object(resources))])).map_err(invalid)?;
        replacements.insert("render".into(), render);
    }
    let metadata = replace(doc, replacements).map_err(invalid)?;
    serde_json::to_vec(&metadata).map_err(invalid)
}

pub(crate) trait BlobReader {
    fn blob(&mut self, value: &Value, field: &str, extension: &str) -> Result<&[u8]>;
    fn limited_blob(&mut self, value: &Value, field: &str, extension: &str, limit: usize) -> Result<&[u8]> {
        let bytes = self.blob(value, field, extension)?;
        if bytes.len() > limit {
            return Err(invalid("blob exceeds resource budget"));
        }
        Ok(bytes)
    }
}

pub(crate) fn decode(bytes: &[u8], reader: &mut impl BlobReader) -> Result<Document> {
    check_hdr_manifest(bytes)?;
    let mut metadata: Value = serde_json::from_slice(bytes).map_err(invalid)?;
    // HDR's typed JSON representation is deliberately strict: no empty placeholders or
    // container-only blob fields may reach the document deserializer.
    let hydrated_hdr = hydrate_hdr_resources(&mut metadata, reader)?;
    let hydrated_text;
    let text = if hydrated_hdr {
        hydrated_text = serde_json::to_string(&metadata).map_err(invalid)?;
        &hydrated_text
    } else {
        std::str::from_utf8(bytes).map_err(invalid)?
    };
    // Migrations and all document validation run on the small parametric metadata.
    // Binary arrays have their own equivalent numeric/index validation below.
    let mut doc = Document::from_json(text)?;
    for (index, feature) in doc.features.iter_mut().enumerate() {
        let value = metadata.get("features").and_then(|v| v.get(index)).ok_or_else(|| invalid("missing feature metadata"))?;
        match &mut feature.kind {
            FeatureKind::Import { step, .. } => {
                if !step.is_empty() {
                    return Err(invalid("ambiguous embedded STEP and blob"));
                }
                *step = std::str::from_utf8(reader.blob(value, "step_blob", "step")?).map_err(invalid)?.into();
            }
            FeatureKind::MeshImport { meshes, .. } => {
                for (index, mesh) in meshes.iter_mut().enumerate() {
                    if !mesh.positions.is_empty() || !mesh.triangles.is_empty() {
                        return Err(invalid("ambiguous embedded mesh and blob"));
                    }
                    let value = value.get("meshes").and_then(|v| v.get(index)).ok_or_else(|| invalid("missing mesh metadata"))?;
                    decode_mesh(reader.blob(value, "mesh_blob", "mesh")?, mesh)?;
                }
            }
            _ => {}
        }
    }
    for (index, canvas) in doc.canvases.iter_mut().enumerate() {
        if !canvas.data.is_empty() || !matches!(canvas.format.as_str(), "png" | "jpeg") {
            return Err(invalid("invalid canvas metadata"));
        }
        let value = metadata.get("canvases").and_then(|v| v.get(index)).ok_or_else(|| invalid("missing canvas metadata"))?;
        let bytes = reader.blob(value, "image_blob", &canvas.format)?;
        if bytes.len() > solidcraft_doc::canvas::MAX_CANVAS_BYTES {
            return Err(invalid("canvas too large"));
        }
        canvas.data = solidcraft_doc::canvas::base64_encode(bytes).into();
    }
    for (index, cloud) in doc.point_clouds.iter_mut().enumerate() {
        if !cloud.points.is_empty() || !cloud.colors.is_empty() {
            return Err(invalid("ambiguous embedded points and blob"));
        }
        let value = metadata.get("point_clouds").and_then(|v| v.get(index)).ok_or_else(|| invalid("missing point cloud metadata"))?;
        decode_cloud(reader.blob(value, "points_blob", "points")?, cloud)?;
    }
    Ok(doc)
}

fn hydrate_hdr_resources(metadata: &mut Value, reader: &mut impl BlobReader) -> Result<bool> {
    use solidcraft_doc::render_resources::{HdrId, MAX_HDR_RESOURCE_BYTES, MAX_HDR_RESOURCES, encode_hdr_base64};

    let Some(resources) = metadata.get_mut("render").and_then(|render| render.get_mut("resources")) else {
        return Ok(false);
    };
    let resources = resources.as_object_mut().ok_or_else(|| invalid("HDR resources must be an object"))?;
    if resources.len() > MAX_HDR_RESOURCES {
        return Err(invalid("at most 64 embedded HDR resources"));
    }
    let mut total = 0usize;
    for (key, value) in resources.iter_mut() {
        let id: HdrId = serde_json::from_value(Value::String(key.clone())).map_err(invalid)?;
        if !value.as_object().is_some_and(|fields| fields.len() == 3)
            || value.get("kind").and_then(Value::as_str) != Some("radiance_hdr")
            || value.get("data").and_then(Value::as_str) != Some("")
        {
            return Err(invalid("HDR metadata requires radiance_hdr, empty data and one blob; inline data plus blob is ambiguous"));
        }
        let remaining = MAX_HDR_RESOURCE_BYTES.saturating_sub(total).min(solidcraft_doc::render_resources::MAX_HDR_BYTES);
        let bytes = reader.limited_blob(value, "hdr_blob", "hdr", remaining)?;
        if HdrId::of(bytes) != id {
            return Err(invalid("HDR resource key does not match its blob hash"));
        }
        total = total.checked_add(bytes.len()).ok_or_else(|| invalid("HDR resource size overflow"))?;
        let encoded = encode_hdr_base64(bytes).map_err(invalid)?;
        let fields = value.as_object_mut().ok_or_else(|| invalid("invalid HDR metadata"))?;
        fields.remove("hdr_blob");
        fields.insert("data".into(), Value::String(encoded));
    }
    Ok(!resources.is_empty())
}
