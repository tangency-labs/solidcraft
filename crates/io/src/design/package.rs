//! Shared authoritative metadata and payload representation for native containers and Git.
//! Storage-specific compression, caching and transport stay outside this module.
use super::*;

/// The authoritative `document.json` bytes. Each payload goes through `blob`, which stores it
/// (a container part or a cloud asset) and returns the reference the metadata keeps. Drawings and
/// plugin data are stored beside the design by the caller; the metadata only names the parts the
/// drawings' images are.
pub(crate) fn encode(doc: &Document, compression: DesignCompression, mut blob: impl FnMut(&[u8], &str, bool) -> Result<Value>) -> Result<Vec<u8>> {
    doc.render.validate().map_err(invalid)?;
    let mut features = Vec::with_capacity(doc.features.len());
    for feature in &doc.features {
        let replacements = match &feature.kind {
            FeatureKind::Import { step, records, .. } => {
                // STEP is text and shrinks several times: always compressed, so an import as large
                // as MAX_STEP_BYTES still fits in a design file.
                let reference = blob(step.as_bytes(), "step", true)?;
                let mut replaced = fields([("step", Value::String(String::new())), ("step_blob", reference)]);
                // An import kept as records: they go in a part of their own, compressed as STEP.
                if let Some(r) = records {
                    let text = solidcraft_kernel::records_text(r)?;
                    replaced.insert("records".into(), Value::Null);
                    replaced.insert("records_blob".into(), blob(text.as_bytes(), "records", true)?);
                }
                replaced
            }
            FeatureKind::MeshImport { meshes, .. } => {
                let mut metadata = Vec::with_capacity(meshes.len());
                for mesh in meshes {
                    let bytes = encode_mesh(mesh)?;
                    // Fast saves store a mesh that fits. One past the file limit is deflated,
                    // or it could never be saved (#6600). Compact deflates every mesh.
                    let compress = compression == DesignCompression::Compact || bytes.len() > max_part();
                    let reference = blob(&bytes, "mesh", compress)?;
                    let mut value = replace(mesh, fields([("positions", json!([])), ("triangles", json!([])), ("corner_normals", json!([]))]))
                        .map_err(invalid)?;
                    value.as_object_mut().ok_or_else(|| invalid("mesh metadata"))?.insert("mesh_blob".into(), reference);
                    metadata.push(value);
                }
                fields([("meshes", Value::Array(metadata))])
            }
            _ => Map::new(),
        };
        // Extra reference fields are added after serializing existing fields.
        let step_blob = replacements.get("step_blob").cloned();
        let records_blob = replacements.get("records_blob").cloned();
        let mut value = replace(feature, replacements).map_err(invalid)?;
        for (key, reference) in [("step_blob", step_blob), ("records_blob", records_blob)] {
            if let Some(reference) = reference {
                value.as_object_mut().ok_or_else(|| invalid("feature metadata"))?.insert(key.into(), reference);
            }
        }
        features.push(value);
    }
    let mut canvases = Vec::with_capacity(doc.canvases.len());
    for canvas in &doc.canvases {
        let bytes = canvas.bytes();
        if !matches!(canvas.format.as_str(), "png" | "jpeg") || bytes.len() > solidcraft_doc::canvas::MAX_CANVAS_BYTES {
            return Err(invalid("invalid canvas format or size"));
        }
        let reference = blob(bytes, &canvas.format, false)?;
        let mut value = serde_json::to_value(canvas).map_err(invalid)?;
        let object = value.as_object_mut().ok_or_else(|| invalid("canvas metadata"))?;
        // The bytes are the blob; an empty `data` keeps the record readable by older releases.
        object.insert("data".into(), json!(""));
        object.insert("image_blob".into(), reference);
        canvases.push(value);
    }
    let mut clouds = Vec::with_capacity(doc.point_clouds.len());
    for cloud in &doc.point_clouds {
        cloud.check().map_err(invalid)?;
        let reference = blob(&encode_cloud(cloud)?, "points", compression == DesignCompression::Compact)?;
        let mut value = replace(cloud, fields([("points", json!("")), ("colors", json!(""))])).map_err(invalid)?;
        let record = value.as_object_mut().ok_or_else(|| invalid("point cloud metadata"))?;
        record.insert("points_blob".into(), reference);
        // Its segments, triangles and properties' numbers are a part of their own; the record keeps
        // their shape with empty numbers.
        if let Some(complex) = encode_complex(&cloud.complex)? {
            record.insert("complex_blob".into(), blob(&complex, "complex", compression == DesignCompression::Compact)?);
        }
        clouds.push(value);
    }
    let mut replacements = fields([("features", Value::Array(features)), ("canvases", Value::Array(canvases)), ("drawings", Value::Null)]);
    if !clouds.is_empty() {
        replacements.insert("point_clouds".into(), Value::Array(clouds));
    }
    if !doc.render.resources.is_empty() || !doc.render.texture_resources.is_empty() {
        let mut changes = Map::new();
        if !doc.render.resources.is_empty() {
            let mut resources = Map::new();
            for (id, resource) in doc.render.resources.iter() {
                let reference = blob(resource.bytes(), "hdr", false)?;
                resources.insert(id.to_string(), json!({"kind":"radiance_hdr","data":"","hdr_blob":reference}));
            }
            changes.insert("resources".into(), Value::Object(resources));
        }
        if !doc.render.texture_resources.is_empty() {
            let mut resources = Map::new();
            for (id, resource) in doc.render.texture_resources.iter() {
                let reference = blob(resource.bytes(), "texture", false)?;
                resources
                    .insert(id.to_string(), json!({"kind":"opaque_texture","data":"","texture_blob":reference,"provenance":resource.provenance()}));
            }
            changes.insert("texture_resources".into(), Value::Object(resources));
        }
        let render = replace(doc.render.as_ref(), changes).map_err(invalid)?;
        replacements.insert("render".into(), render);
    }
    let mut metadata = replace(doc, replacements).map_err(invalid)?;
    // The drawings have entries of their own, and plugin data goes beside the design, a part each,
    // as it came: `document.json` is the same with them or without.
    if let Some(object) = metadata.as_object_mut() {
        object.remove("drawings");
        object.remove("plugin_data");
        // The parts the drawings' images are, named where a release that can't read the drawings'
        // records (0.45.0) looks before it keeps a part, so its save keeps them.
        let mut parts: Vec<String> = doc
            .drawings
            .iter()
            .flat_map(|d| {
                d.images
                    .iter()
                    .map(|i| solidcraft_doc::blob::part_name(&i.data, &i.format))
                    .chain(d.newer_file.iter().flat_map(|file| file.part_names().map(String::from)))
            })
            .collect();
        parts.sort();
        parts.dedup();
        if !parts.is_empty() {
            object.insert("drawing_parts".into(), json!(parts));
        }
    }
    solidcraft_doc::newer_file::restore(doc, &mut metadata);
    let metadata = serde_json::to_vec(&metadata).map_err(invalid)?;
    Ok(metadata)
}

/// Where the payloads the metadata names are read from: a container's parts or a snapshot's assets.
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
    decode_with(bytes, reader, Decoding { newer: false, newer_container: None, record_bodies: MAX_RECORD_BODIES }, |_, _, _, _| Ok(()))
}

/// How [`decode_with`] reads one document.
pub(super) struct Decoding {
    /// A newer build's document: what doesn't read here is kept (`newer_file`).
    pub newer: bool,
    /// What a container this build doesn't know says it is, when it is one.
    pub newer_container: Option<String>,
    /// The most bodies the import records parts may hold together ([`MAX_RECORD_BODIES`]).
    pub record_bodies: usize,
}

/// Native containers keep what a newer build wrote and read the parts beside the design (`keep`)
/// before the known payloads are decoded.
pub(super) fn decode_with<R: BlobReader>(
    bytes: &[u8],
    reader: &mut R,
    decoding: Decoding,
    keep: impl FnOnce(&mut Document, &Value, &mut R, &mut RecordsParts) -> Result<()>,
) -> Result<Document> {
    let Decoding { newer, newer_container, record_bodies } = decoding;
    let mut metadata: Value = serde_json::from_slice(bytes).map_err(invalid)?;
    // What the drawings' images are, for a release that can't read the drawings: this build reads
    // them from the drawings and writes the list again on every save.
    let named_drawing_parts = metadata.as_object_mut().and_then(|object| object.remove("drawing_parts")).is_some();
    // HDR's typed JSON representation is deliberately strict: no empty placeholders or
    // container-only blob fields may reach the document deserializer. A newer build's section that
    // doesn't pass stays as it is, which the document reader then leaves out.
    // Their files are the container's parts, handed to the reader as bytes: never text.
    let mut parts = solidcraft_doc::render_resources::ContainerParts::default();
    let hydrated = if newer {
        let mut copy = metadata.clone();
        let mut found = solidcraft_doc::render_resources::ContainerParts::default();
        let done = check_hdr_manifest(bytes).and_then(|()| check_texture_manifest(bytes)).and_then(|()| {
            let hdr = hydrate_hdr_resources(&mut copy, reader, &mut found)?;
            Ok(hydrate_texture_resources(&mut copy, reader, &mut found)? || hdr)
        });
        if done.is_ok() {
            metadata = copy;
            parts = found;
        }
        done.unwrap_or(false)
    } else {
        check_hdr_manifest(bytes)?;
        check_texture_manifest(bytes)?;
        let hdr = hydrate_hdr_resources(&mut metadata, reader, &mut parts)?;
        hydrate_texture_resources(&mut metadata, reader, &mut parts)? || hdr
    };
    // A cloud's points are in its blob, so its segments, triangles and properties are read and
    // checked with them below (`decode_cloud`), not against the empty points the metadata holds
    // (#14572). The metadata keeps them for what a newer build's file holds. A newer build's file
    // reads them as before: what doesn't read here is kept whole (`newer_file`), unknown fields too.
    let complexes = if newer { Vec::new() } else { take_cloud_complexes(&mut metadata) };
    let hydrated_text;
    let text = if hydrated || named_drawing_parts || !complexes.is_empty() {
        hydrated_text = serde_json::to_string(&metadata).map_err(invalid)?;
        &hydrated_text
    } else {
        std::str::from_utf8(bytes).map_err(invalid)?
    };
    // Migrations and all document validation run on the small parametric metadata.
    // Binary arrays have their own equivalent numeric/index validation below.
    let mut doc = solidcraft_doc::render_resources::with_container_parts(parts, || Document::from_json(text))?;
    if !doc.drawings.is_empty() {
        return Err(invalid("ambiguous drawings in document.json and in their own entries"));
    }
    for (index, complex) in complexes {
        if let Some(cloud) = doc.point_clouds.get_mut(index) {
            // A record with a part has empty numbers; an older one has them inline, base64.
            cloud.complex = serde::Deserialize::deserialize(&complex).map_err(invalid)?;
            if let Some(record) = metadata.get("point_clouds").and_then(|v| v.get(index)).filter(|v| v.get("complex_blob").is_some()) {
                decode_complex(reader.blob(record, "complex_blob", "complex")?, &mut cloud.complex)?;
            }
        }
        if let Some(cloud) = metadata.get_mut("point_clouds").and_then(|v| v.get_mut(index)).and_then(Value::as_object_mut) {
            cloud.insert("complex".into(), complex);
        }
    }
    if let Some(says) = newer_container
        && doc.newer_file.is_none()
    {
        let current = solidcraft_doc::format::FORMAT;
        let file = solidcraft_doc::newer_file::newer_file(&solidcraft_doc::format::Newer {
            format: current,
            reads_up_to: current,
            unknown_changes: vec![says],
        });
        doc.newer_file = Some(Arc::new(file));
    }
    let mut records_parts = RecordsParts::new(doc.newer_file.is_some(), record_bodies);
    keep(&mut doc, &metadata, reader, &mut records_parts)?;
    for (index, feature) in doc.features.iter_mut().enumerate() {
        let value = metadata.get("features").and_then(|v| v.get(index)).ok_or_else(|| invalid("missing feature metadata"))?;
        match &mut feature.kind {
            FeatureKind::Import { step, records, .. } => {
                if !step.is_empty() {
                    return Err(invalid("ambiguous embedded STEP and blob"));
                }
                *step = std::str::from_utf8(reader.blob(value, "step_blob", "step")?).map_err(invalid)?.into();
                if value.get("records_blob").is_some() {
                    if records.is_some() {
                        return Err(invalid("ambiguous embedded import records and blob"));
                    }
                    *records = Some(records_parts.get(reader, value)?);
                }
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
        canvas.data = bytes.into();
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

/// Takes each HDR's part out for the reader (`parts`), leaving its record `{kind, data: ""}`.
fn hydrate_hdr_resources(
    metadata: &mut Value,
    reader: &mut impl BlobReader,
    parts: &mut solidcraft_doc::render_resources::ContainerParts,
) -> Result<bool> {
    use solidcraft_doc::render_resources::{HdrId, MAX_HDR_RESOURCE_BYTES, MAX_HDR_RESOURCES};

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
        // The record keeps naming its part: a newer build's section this build keeps as it came
        // still says which part it needs, so saving keeps the part too.
        parts.hdr.insert(id, bytes.to_vec());
    }
    Ok(!resources.is_empty())
}

/// Takes each texture's part out for the reader (`parts`), as [`hydrate_hdr_resources`].
fn hydrate_texture_resources(
    metadata: &mut Value,
    reader: &mut impl BlobReader,
    parts: &mut solidcraft_doc::render_resources::ContainerParts,
) -> Result<bool> {
    use solidcraft_doc::{
        render_resources::HdrId,
        render_texture_resources::{MAX_TEXTURE_BYTES, MAX_TEXTURE_RESOURCE_BYTES, MAX_TEXTURE_RESOURCES},
    };
    let Some(resources) = metadata.get_mut("render").and_then(|render| render.get_mut("texture_resources")) else {
        return Ok(false);
    };
    let resources = resources.as_object_mut().ok_or_else(|| invalid("texture resources must be an object"))?;
    if resources.len() > MAX_TEXTURE_RESOURCES {
        return Err(invalid("at most 32 embedded texture resources"));
    }
    let mut total = 0usize;
    for (key, value) in resources.iter_mut() {
        let id: HdrId = serde_json::from_value(Value::String(key.clone())).map_err(invalid)?;
        if !value.as_object().is_some_and(|fields| fields.len() == 4)
            || value.get("kind").and_then(Value::as_str) != Some("opaque_texture")
            || value.get("data").and_then(Value::as_str) != Some("")
        {
            return Err(invalid("texture metadata requires opaque_texture, empty data, provenance and one blob"));
        }
        let remaining = MAX_TEXTURE_RESOURCE_BYTES.saturating_sub(total).min(MAX_TEXTURE_BYTES);
        let bytes = reader.limited_blob(value, "texture_blob", "texture", remaining)?;
        if HdrId::of(bytes) != id {
            return Err(invalid("texture resource key does not match its blob hash"));
        }
        total = total.checked_add(bytes.len()).ok_or_else(|| invalid("texture resource size overflow"))?;
        parts.textures.insert(id, bytes.to_vec());
    }
    Ok(!resources.is_empty())
}
