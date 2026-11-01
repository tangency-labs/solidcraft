//! Versioned design packages. Entries are never extracted to filesystem paths. The optional
//! model cache is only read back by the build that wrote it, for the exact document it was
//! written with.
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read, Seek, SeekFrom, Write};
use std::sync::Arc;

mod drawing;
mod native_cache;
mod thumbnail;
pub use drawing::{FileKind, file_kind, read_drawing, read_drawing_file, write_drawing_container};
pub use native_cache::read_design_geometry;
pub use thumbnail::{THUMBNAIL_SIZE, read_design_thumbnail, read_design_thumbnail_file};

use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use solidcraft_doc::{Document, FeatureKind, MeshData};

use crate::{IoError, Result, design_metadata::replace};

const VERSION: &str = "solidcraft-container/1";
const LEGACY_VERSION: &str = "solvecraft-container/1";
const PREVIEW_VERSION: &str = "solidcraft-preview/1";
const MAX_ENTRIES: usize = 10_000;
/// The design file limit in force ([`crate::limits`]): the largest design file read or saved.
fn max_design() -> usize {
    crate::limits::max_design_bytes()
}
/// An embedded STEP source may be as large as an import accepts. It is always stored
/// Deflate-compressed, so the file itself stays within the design file limit (#3221).
fn max_step_part() -> usize {
    crate::limits::max_step_bytes()
}
/// Deflate level for STEP sources. Level 1 uses fixed Huffman codes: a 270 MiB STEP export
/// shrank to 70 MB in 1.6 s, at level 2 to 47 MB in 3.1 s; higher levels gain little more.
const STEP_LEVEL: i64 = 2;
/// An embedded mesh may be as large as a STEP source. One bigger than the design file is
/// stored Deflate-compressed, so a compressible large import can be saved (#6600). The file
/// itself stays within the design file limit.
fn max_mesh_part() -> usize {
    crate::limits::max_step_bytes()
}
/// Every other part is bounded by the file size limit.
fn max_part() -> usize {
    max_design()
}
fn max_expanded() -> usize {
    max_step_part().saturating_add(max_design())
}
const MESH_MAGIC: &[u8; 8] = b"SCMESH01";
const POINTS_MAGIC: &[u8; 8] = b"SCPNTS01";
const MAX_PREVIEW_BYTES: usize = 8 << 20;
const MAX_PREVIEW_TRIANGLES: usize = 100_000;

/// Untrusted, display-only evaluated meshes. Never used for editing, picking or measurement.
#[derive(Default)]
pub struct DesignPreview {
    pub meshes: Vec<MeshData>,
}

/// Fast favors interactive file operations; Compact trades CPU time for space.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DesignCompression {
    #[default]
    Fast,
    Compact,
}

fn invalid(message: impl std::fmt::Display) -> IoError {
    IoError::Invalid(format!("design container: {message}"))
}

/// Detect the package signature independently of the file name.
pub fn is_design_container(bytes: &[u8]) -> bool {
    bytes.starts_with(b"PK\x03\x04")
}

fn fields(pairs: impl IntoIterator<Item = (&'static str, Value)>) -> Map<String, Value> {
    pairs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect()
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

struct PackageWriter {
    zip: zip::ZipWriter<Cursor<Vec<u8>>>,
    names: BTreeSet<String>,
    expanded: usize,
    resolved: usize,
}

/// The uncompressed size a part may have. STEP and mesh blobs may be larger than the
/// design file because they are stored compressed when they would not fit otherwise.
fn part_limit(name: &str) -> usize {
    if name.ends_with(".step") {
        max_step_part()
    } else if name.ends_with(".mesh") {
        max_mesh_part()
    } else {
        max_part()
    }
}

impl PackageWriter {
    fn new() -> PackageWriter {
        PackageWriter { zip: zip::ZipWriter::new(Cursor::new(Vec::new())), names: BTreeSet::new(), expanded: 0, resolved: 0 }
    }
    fn finish(self) -> Result<Vec<u8>> {
        Ok(self.zip.finish().map_err(invalid)?.into_inner())
    }
    fn part(&mut self, name: &str, bytes: &[u8], compressed: bool) -> Result<()> {
        if bytes.len() > part_limit(name) {
            return Err(invalid("part too large"));
        }
        if !self.names.insert(name.to_owned()) {
            return Ok(());
        }
        self.expanded = self.expanded.checked_add(bytes.len()).ok_or_else(|| invalid("expanded size overflow"))?;
        if self.expanded > max_expanded() || self.names.len() > MAX_ENTRIES {
            return Err(invalid("package too large"));
        }
        let method = if compressed { zip::CompressionMethod::Deflated } else { zip::CompressionMethod::Stored };
        // A mesh past the file limit uses the STEP level: level 1 of a large payload leaves
        // more on disk, and this part exists only because compression is what makes it fit.
        let level = if name.ends_with(".step") || (name.ends_with(".mesh") && bytes.len() > max_part()) { STEP_LEVEL } else { 1 };
        // A part of 4 GiB or more (only with a raised file limit, #4086) needs a ZIP64 entry.
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(method)
            .compression_level(compressed.then_some(level))
            .large_file(bytes.len() as u64 >= u64::from(u32::MAX));
        self.zip.start_file(name, options).map_err(invalid)?;
        self.zip.write_all(bytes).map_err(invalid)
    }
    /// Would `entries` more optional parts of `extra` bytes, stored uncompressed, still keep
    /// the file within `budget` and its expanded size within the reader's budget?
    fn room(&self, entries: usize, extra: usize, budget: usize) -> bool {
        self.names.len().saturating_add(entries) <= MAX_ENTRIES && extra <= self.space(entries, budget)
    }
    /// The bytes `entries` more optional parts may hold together ([`PackageWriter::room`]).
    fn space(&self, entries: usize, budget: usize) -> usize {
        if self.names.len().saturating_add(entries) > MAX_ENTRIES {
            return 0;
        }
        // The bytes written so far, plus what the open compressed part may still hold back.
        let written = self.zip.get_ref().map_or(usize::MAX, |c| c.get_ref().len()).saturating_add(1 << 20);
        let directory = self.names.len().saturating_add(entries).saturating_mul(256).saturating_add(65_536);
        budget.saturating_sub(written.saturating_add(directory)).min(max_expanded().saturating_sub(self.expanded))
    }
    fn blob(&mut self, bytes: &[u8], extension: &str, compressed: bool) -> Result<Value> {
        self.resolved =
            self.resolved.checked_add(bytes.len()).filter(|&n| n <= max_expanded()).ok_or_else(|| invalid("resolved payloads too large"))?;
        let name = format!("blobs/{}.{extension}", digest(bytes));
        self.part(&name, bytes, compressed)?;
        Ok(Value::String(name))
    }
}

/// Counts what is written without keeping it.
#[derive(Default)]
struct Measure {
    position: u64,
    length: u64,
}

impl Write for Measure {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.position = self.position.saturating_add(bytes.len() as u64);
        self.length = self.length.max(self.position);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Seek for Measure {
    fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        let target = match position {
            SeekFrom::Start(n) => Some(n),
            SeekFrom::End(n) => self.length.checked_add_signed(n),
            SeekFrom::Current(n) => self.position.checked_add_signed(n),
        };
        self.position = target.ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "seek out of range"))?;
        Ok(self.position)
    }
}

/// Room kept in a design file for everything besides one large embedded STEP source.
const STEP_RESERVE: usize = 4 << 20;

/// Can an import's STEP text be saved in a design? It must be no larger than an import
/// accepts, and, compressed as a save stores it, leave room in a design file for the rest of
/// the design. Checked when the file is imported, so it never imports and then fails to save.
pub fn check_step_storable(step: &str) -> Result<()> {
    let limit = max_design().saturating_sub(STEP_RESERVE);
    crate::limits::check(crate::limits::Kind::Step, step.len() as u64).map_err(|e| IoError::Invalid(format!("STEP {e}")))?;
    // Deflate adds at most a few bytes per 16 KiB block: smaller text always fits.
    if step.len() <= limit - (limit >> 10) {
        return Ok(());
    }
    let mut zip = zip::ZipWriter::new(Measure::default());
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .compression_level(Some(STEP_LEVEL))
        .large_file(step.len() as u64 >= u64::from(u32::MAX));
    zip.start_file("blobs/source.step", options).map_err(invalid)?;
    zip.write_all(step.as_bytes()).map_err(invalid)?;
    let compressed = usize::try_from(zip.finish().map_err(invalid)?.length).unwrap_or(usize::MAX);
    if compressed > limit {
        return Err(IoError::Invalid(format!(
            "STEP file too large to save in a design: {} MB of STEP text compresses to {} MB, over the {} MB a design file may hold. \
             The design file limit counts here too: {}",
            step.len() >> 20,
            compressed >> 20,
            limit >> 20,
            crate::limits::how_to_raise()
        )));
    }
    Ok(())
}

/// Write a compact container, deduplicating payloads by SHA-256. Mesh arrays are
/// stored directly for low save/open latency unless a mesh exceeds the file limit;
/// larger meshes, STEP sources and small metadata use Deflate. Use the explicit
/// Compact option to compress smaller meshes too.
pub fn write_design_container(doc: &Document) -> Result<Vec<u8>> {
    write_design_container_with_compression(doc, DesignCompression::Fast)
}

/// Write a container with an explicit payload compression policy.
pub fn write_design_container_with_compression(doc: &Document, compression: DesignCompression) -> Result<Vec<u8>> {
    write_design_container_with_preview(doc, compression, None)
}

/// Optional display cache failure never prevents saving the authoritative design.
pub fn write_design_container_with_preview(doc: &Document, compression: DesignCompression, preview: Option<&DesignPreview>) -> Result<Vec<u8>> {
    write_design_container_with_cache(doc, compression, preview, None)
}

/// Save an optional model cache (the evaluated timeline's bodies) without making document
/// saving depend on it. Display meshes remain separately opt-in; no evaluation or meshing
/// happens here.
pub fn write_design_container_with_cache(
    doc: &Document,
    compression: DesignCompression,
    preview: Option<&DesignPreview>,
    geometry: Option<&solidcraft_doc::SavedModel>,
) -> Result<Vec<u8>> {
    write_design_container_with_thumbnail(doc, compression, preview, geometry, None)
}

/// Include a bounded portable PNG picture when one is already available.
/// Unsupported headers or oversized pictures are omitted without changing the design.
pub fn write_design_container_with_thumbnail(
    doc: &Document,
    compression: DesignCompression,
    preview: Option<&DesignPreview>,
    geometry: Option<&solidcraft_doc::SavedModel>,
    thumbnail: Option<&[u8]>,
) -> Result<Vec<u8>> {
    write_container(doc, compression, preview, geometry, thumbnail, max_design())
}

/// [`write_design_container_with_thumbnail`] within `budget` bytes for the optional caches (the
/// container's budget; tests pass less).
fn write_container(
    doc: &Document,
    compression: DesignCompression,
    preview: Option<&DesignPreview>,
    geometry: Option<&solidcraft_doc::SavedModel>,
    thumbnail: Option<&[u8]>,
    budget: usize,
) -> Result<Vec<u8>> {
    doc.render.validate().map_err(invalid)?;
    let mut writer = PackageWriter::new();
    let mut features = Vec::with_capacity(doc.features.len());
    for feature in &doc.features {
        let replacements = match &feature.kind {
            FeatureKind::Import { step, .. } => {
                // STEP is text and shrinks several times: always compressed, so an import as large
                // as MAX_STEP_BYTES still fits in a design file.
                let blob = writer.blob(step.as_bytes(), "step", true)?;
                fields([("step", Value::String(String::new())), ("step_blob", blob)])
            }
            FeatureKind::MeshImport { meshes, .. } => {
                let mut metadata = Vec::with_capacity(meshes.len());
                for mesh in meshes {
                    let bytes = encode_mesh(mesh)?;
                    // Fast saves store a mesh that fits. One past the file limit is deflated,
                    // or it could never be saved (#6600). Compact deflates every mesh.
                    let compress = compression == DesignCompression::Compact || bytes.len() > max_part();
                    let blob = writer.blob(&bytes, "mesh", compress)?;
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
        let blob = writer.blob(&bytes, &canvas.format, false)?;
        let mut value = replace(canvas, fields([("data", json!(""))])).map_err(invalid)?;
        value.as_object_mut().ok_or_else(|| invalid("canvas metadata"))?.insert("image_blob".into(), blob);
        canvases.push(value);
    }
    let mut clouds = Vec::with_capacity(doc.point_clouds.len());
    for cloud in &doc.point_clouds {
        cloud.check().map_err(invalid)?;
        let blob = writer.blob(&encode_cloud(cloud)?, "points", compression == DesignCompression::Compact)?;
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
            let blob = writer.blob(resource.bytes(), "hdr", false)?;
            resources.insert(id.to_string(), json!({"kind":"radiance_hdr","data":"","hdr_blob":blob}));
        }
        let render = replace(doc.render.as_ref(), fields([("resources", Value::Object(resources))])).map_err(invalid)?;
        replacements.insert("render".into(), render);
    }
    let metadata = replace(doc, replacements).map_err(invalid)?;
    let metadata = serde_json::to_vec(&metadata).map_err(invalid)?;
    writer.part("document.json", &metadata, true)?;
    writer.part("container.json", &serde_json::to_vec(&json!({"format": VERSION})).map_err(invalid)?, false)?;
    // The model cache is optional: one that does not fit the container keeps fewer states, or is
    // left out, never a failed save.
    let cache = geometry.and_then(|g| native_cache::encode_model(g, &metadata, writer.space(2, budget)).ok());
    if let Some((manifest, data)) = cache {
        let [manifest_entry, payload_entry] = native_cache::ENTRIES;
        // Deflated whatever the policy: the encoded bodies shrink about fourfold to tenfold, faster
        // than the extra bytes would be written and read.
        writer.part(manifest_entry, &manifest, true)?;
        writer.part(payload_entry, &data, true)?;
    }
    if let Some((manifest, data)) = preview.and_then(|p| encode_preview(p, &metadata).ok())
        && writer.room(2, manifest.len().saturating_add(data.len()), budget)
    {
        writer.part("cache/preview.json", &manifest, false)?;
        writer.part("cache/preview.bin", &data, false)?;
    }
    if let Some(png) = thumbnail {
        thumbnail::write(&mut writer, png, &metadata)?;
    }
    let bytes = writer.finish()?;
    if bytes.len() > max_design() {
        return Err(invalid(format!(
            "the design would be {} MB, over the {} MB a design file may hold. {}",
            bytes.len() >> 20,
            max_design() >> 20,
            crate::limits::how_to_raise()
        )));
    }
    Ok(bytes)
}

fn validate_mesh(mesh: &MeshData) -> Result<()> {
    if !mesh.positions.len().is_multiple_of(3)
        || !mesh.triangles.len().is_multiple_of(3)
        || mesh.positions.iter().any(|v| !v.is_finite() || v.abs() > 1e9)
        || mesh.triangles.iter().any(|&v| v as usize >= mesh.positions.len() / 3)
    {
        return Err(invalid("invalid mesh coordinates or indices"));
    }
    Ok(())
}

/// Encoded blob length, or `None` when it would pass the mesh part limit or overflow.
fn mesh_encoded_len(positions: usize, triangles: usize) -> Option<usize> {
    positions
        .checked_mul(8)
        .and_then(|v| triangles.checked_mul(4).and_then(|t| v.checked_add(t)))
        .and_then(|v| v.checked_add(24))
        .filter(|&n| n <= max_mesh_part())
}

fn encode_mesh(mesh: &MeshData) -> Result<Vec<u8>> {
    let size = mesh_encoded_len(mesh.positions.len(), mesh.triangles.len()).ok_or_else(|| invalid("mesh too large"))?;
    validate_mesh(mesh)?;
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(size).map_err(|_| invalid("could not reserve mesh blob"))?;
    bytes.extend_from_slice(MESH_MAGIC);
    bytes.extend_from_slice(&(mesh.positions.len() as u64).to_le_bytes());
    bytes.extend_from_slice(&(mesh.triangles.len() as u64).to_le_bytes());
    for value in mesh.positions.iter() {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    for value in mesh.triangles.iter() {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    Ok(bytes)
}

fn decode_mesh(bytes: &[u8], mesh: &mut MeshData) -> Result<()> {
    if bytes.get(..8) != Some(MESH_MAGIC.as_slice()) {
        return Err(invalid("unsupported mesh blob version"));
    }
    let count = |range: std::ops::Range<usize>| -> Result<usize> {
        let array: [u8; 8] = bytes.get(range).ok_or_else(|| invalid("truncated mesh blob"))?.try_into().map_err(invalid)?;
        usize::try_from(u64::from_le_bytes(array)).map_err(invalid)
    };
    let positions = count(8..16)?;
    let triangles = count(16..24)?;
    let middle = positions.checked_mul(8).and_then(|n| n.checked_add(24)).ok_or_else(|| invalid("mesh size overflow"))?;
    let total = triangles.checked_mul(4).and_then(|n| n.checked_add(middle)).ok_or_else(|| invalid("mesh size overflow"))?;
    if total != bytes.len() || !positions.is_multiple_of(3) || !triangles.is_multiple_of(3) {
        return Err(invalid("invalid mesh blob lengths"));
    }
    mesh.positions =
        bytes.get(24..middle).ok_or_else(|| invalid("truncated mesh"))?.as_chunks::<8>().0.iter().map(|&v| f64::from_le_bytes(v)).collect();
    mesh.triangles =
        bytes.get(middle..).ok_or_else(|| invalid("truncated mesh"))?.as_chunks::<4>().0.iter().map(|&v| u32::from_le_bytes(v)).collect();
    validate_mesh(mesh)
}

/// A point cloud's points and colours: the magic, the point count and whether there are
/// colours (u64 each, little-endian), then x, y, z per point (f32) and r, g, b per point.
fn encode_cloud(cloud: &solidcraft_doc::point_cloud::PointCloud) -> Result<Vec<u8>> {
    let n = cloud.points.len();
    let colored = !cloud.colors.is_empty();
    let size = n
        .checked_mul(if colored { 15 } else { 12 })
        .and_then(|b| b.checked_add(24))
        .filter(|&b| b <= crate::limits::max_design_bytes())
        .ok_or_else(|| invalid("point cloud too large"))?;
    let mut bytes = Vec::with_capacity(size);
    bytes.extend_from_slice(POINTS_MAGIC);
    bytes.extend_from_slice(&(n as u64).to_le_bytes());
    bytes.extend_from_slice(&u64::from(colored).to_le_bytes());
    bytes.extend_from_slice(&cloud.points.to_bytes());
    if colored {
        bytes.extend_from_slice(&cloud.colors.0);
    }
    Ok(bytes)
}

fn decode_cloud(bytes: &[u8], cloud: &mut solidcraft_doc::point_cloud::PointCloud) -> Result<()> {
    use solidcraft_doc::point_cloud::{Colors, MAX_POINTS, Points};
    if bytes.get(..8) != Some(POINTS_MAGIC.as_slice()) {
        return Err(invalid("unsupported point cloud blob version"));
    }
    let word = |range: std::ops::Range<usize>| -> Result<u64> {
        let array: [u8; 8] = bytes.get(range).ok_or_else(|| invalid("truncated point cloud blob"))?.try_into().map_err(invalid)?;
        Ok(u64::from_le_bytes(array))
    };
    let n = usize::try_from(word(8..16)?).ok().filter(|&n| n <= MAX_POINTS).ok_or_else(|| invalid("point cloud too large"))?;
    let colored = match word(16..24)? {
        0 => false,
        1 => true,
        _ => return Err(invalid("invalid point cloud blob")),
    };
    let middle = 24 + n * 12;
    if bytes.len() != middle + if colored { n * 3 } else { 0 } {
        return Err(invalid("invalid point cloud blob lengths"));
    }
    cloud.points = Points::from_bytes(bytes.get(24..middle).ok_or_else(|| invalid("truncated point cloud"))?)
        .ok_or_else(|| invalid("invalid point coordinates"))?;
    cloud.colors = if colored {
        Colors::from_bytes(bytes.get(middle..).ok_or_else(|| invalid("truncated point cloud"))?).ok_or_else(|| invalid("invalid point colours"))?
    } else {
        Colors::default()
    };
    cloud.check().map_err(invalid)
}

struct PackageReader<R: Read + Seek> {
    zip: zip::ZipArchive<R>,
    blobs: BTreeMap<String, Vec<u8>>,
    hydrated: usize,
    document: Option<Arc<[u8]>>,
}

impl<R: Read + Seek> PackageReader<R> {
    fn new(mut source: R) -> Result<Self> {
        let length = source.seek(SeekFrom::End(0)).map_err(invalid)?;
        crate::limits::check(crate::limits::Kind::Design, length).map_err(invalid)?;
        let tail_length = length.min(65_557) as usize;
        source.seek(SeekFrom::Start(length - tail_length as u64)).map_err(invalid)?;
        let mut tail = vec![0; tail_length];
        source.read_exact(&mut tail).map_err(invalid)?;
        // zip's name index can coalesce duplicate central-directory entries. Our
        // bounded format does not need ZIP64/multiple disks: verify the declared
        // entry count as well so duplicate names cannot silently select a payload.
        let eocd = tail.windows(4).rposition(|v| v == b"PK\x05\x06").ok_or_else(|| invalid("missing ZIP directory"))?;
        let end = tail.get(eocd..).filter(|v| v.len() >= 22).ok_or_else(|| invalid("truncated ZIP directory"))?;
        let u16_at = |offset| -> Result<u16> {
            let array: [u8; 2] = end.get(offset..offset + 2).ok_or_else(|| invalid("truncated ZIP directory"))?.try_into().map_err(invalid)?;
            Ok(u16::from_le_bytes(array))
        };
        let declared = u16_at(10)? as usize;
        if declared > MAX_ENTRIES || u16_at(4)? != 0 || u16_at(6)? != 0 || u16_at(8)? as usize != declared || end.len() != 22 + u16_at(20)? as usize {
            return Err(invalid("too many entries or unsupported ZIP directory"));
        }
        // A design file is a ZIP with nothing in front of it. Saying so skips zip's search for a
        // prefix, which reads back through the stored parts and would load a large embedded
        // source before the preview can be shown.
        let config = zip::read::Config { archive_offset: zip::read::ArchiveOffset::Known(0) };
        let mut zip = zip::ZipArchive::with_config(config, source).map_err(invalid)?;
        if u16_at(4)? != 0
            || u16_at(6)? != 0
            || u16_at(8)? as usize != zip.len()
            || u16_at(10)? as usize != zip.len()
            || end.len() != 22 + u16_at(20)? as usize
        {
            return Err(invalid("duplicate entries or unsupported ZIP directory"));
        }
        let mut names = BTreeSet::new();
        let mut total = 0u64;
        for i in 0..zip.len() {
            let part = zip.by_index_raw(i).map_err(invalid)?;
            let name = part.name().map_err(invalid)?.into_owned();
            if name.is_empty()
                || name.contains('\\')
                || name.split('/').any(|c| c.is_empty() || c == "." || c == "..")
                || !names.insert(name.to_owned())
            {
                return Err(invalid("duplicate or invalid archive entry"));
            }
            total = total.checked_add(part.size()).ok_or_else(|| invalid("expanded size overflow"))?;
            let limit = part_limit(&name);
            if part.size() > limit as u64 || total > max_expanded() as u64 {
                return Err(invalid("expanded package too large"));
            }
        }
        Ok(Self { zip, blobs: BTreeMap::new(), hydrated: 0, document: None })
    }
    // Bind provisional caches and authoritative reconstruction to identical metadata,
    // even if another process overwrites the open file in place. Blobs remain hash-checked.
    fn document(&mut self, limit: usize) -> Result<Arc<[u8]>> {
        if self.document.is_none() {
            self.document = Some(self.limited_part("document.json", limit)?.into());
        }
        self.document.as_ref().filter(|d| d.len() <= limit).cloned().ok_or_else(|| invalid("metadata budget"))
    }
    fn has_part(&self, name: &str) -> bool {
        self.zip.index_for_name(name).is_some()
    }
    fn limited_part(&mut self, name: &str, limit: usize) -> Result<Vec<u8>> {
        let part = self.zip.by_name(name).map_err(|e| invalid(format!("required part `{name}`: {e}")))?;
        let expected = part.size();
        if expected > limit as u64 {
            return Err(invalid("part too large"));
        }
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(expected as usize).map_err(|_| invalid("could not reserve required part"))?;
        // A compressed part may expand beyond its declared size (a decompression bomb): stop
        // one byte past it instead of at the limit.
        part.take(expected + 1).read_to_end(&mut bytes).map_err(invalid)?;
        if bytes.len() > limit || bytes.len() as u64 != expected {
            return Err(invalid("part too large or length mismatch"));
        }
        Ok(bytes)
    }
    fn blob(&mut self, value: &Value, field: &str, extension: &str) -> Result<&[u8]> {
        self.limited_blob(value, field, extension, part_limit(&format!("blob.{extension}")))
    }
    fn limited_blob(&mut self, value: &Value, field: &str, extension: &str, limit: usize) -> Result<&[u8]> {
        let name = value.get(field).and_then(Value::as_str).ok_or_else(|| invalid(format!("missing `{field}` reference")))?;
        let hash = name
            .strip_prefix("blobs/")
            .and_then(|n| n.strip_suffix(&format!(".{extension}")))
            .filter(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
            .ok_or_else(|| invalid("invalid blob reference"))?;
        if !self.blobs.contains_key(name) {
            let bytes = self.limited_part(name, limit)?;
            if digest(&bytes) != hash {
                return Err(invalid(format!("blob hash mismatch: {name}")));
            }
            self.blobs.insert(name.to_owned(), bytes);
        }
        let bytes = self.blobs.get(name).ok_or_else(|| invalid("missing blob"))?;
        if bytes.len() > limit {
            return Err(invalid("blob exceeds its resource budget"));
        }
        // Count repeated references too: a tiny archive must not expand into unbounded
        // owned document strings/arrays when the same blob is referenced repeatedly.
        self.hydrated =
            self.hydrated.checked_add(bytes.len()).filter(|&n| n <= max_expanded()).ok_or_else(|| invalid("resolved payloads too large"))?;
        Ok(bytes)
    }
}

fn encode_preview(preview: &DesignPreview, document: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
    if preview.meshes.is_empty() || preview.meshes.len() > 1024 {
        return Err(invalid("preview body count"));
    }
    let mut data = Vec::new();
    let mut entries = Vec::new();
    let mut triangles = 0usize;
    for mesh in &preview.meshes {
        triangles = triangles.saturating_add(mesh.triangles.len() / 3);
        let size = mesh.positions.len().saturating_mul(8).saturating_add(mesh.triangles.len().saturating_mul(4)).saturating_add(24);
        if data.len().saturating_add(size) > MAX_PREVIEW_BYTES || triangles > MAX_PREVIEW_TRIANGLES || mesh.name.len() > 1024 {
            return Err(invalid("preview budget"));
        }
        let bytes = encode_mesh(mesh)?;
        entries.push(json!({"name":mesh.name,"color":mesh.color,"offset":data.len(),"length":bytes.len()}));
        data.extend_from_slice(&bytes);
    }
    let manifest = serde_json::to_vec(&json!({
        "format":PREVIEW_VERSION, "build":env!("CARGO_PKG_VERSION"),
        "document":digest(document), "payload":digest(&data), "meshes":entries
    }))
    .map_err(invalid)?;
    if manifest.len() > 65_536 {
        return Err(invalid("preview manifest budget"));
    }
    Ok((manifest, data))
}

/// Read only an optional provisional display cache; any corruption/incompatibility is a miss.
/// The normal design reader must still validate and rebuild before enabling the design.
pub fn read_design_preview(bytes: &[u8]) -> Option<DesignPreview> {
    if bytes.len() > max_design() || !is_design_container(bytes) {
        return None;
    }
    DesignContainerReader::new(Cursor::new(bytes)).ok()?.preview()
}

fn read_preview<R: Read + Seek>(reader: &mut PackageReader<R>) -> Result<DesignPreview> {
    let container: Value = serde_json::from_slice(&reader.limited_part("container.json", 4096)?).map_err(invalid)?;
    if !matches!(container.get("format").and_then(Value::as_str), Some(VERSION | LEGACY_VERSION)) {
        return Err(invalid("unsupported container version"));
    }
    let manifest: Value = serde_json::from_slice(&reader.limited_part("cache/preview.json", 65_536)?).map_err(invalid)?;
    if manifest.get("format").and_then(Value::as_str) != Some(PREVIEW_VERSION)
        || manifest.get("build").and_then(Value::as_str) != Some(env!("CARGO_PKG_VERSION"))
    {
        return Err(invalid("incompatible preview"));
    }
    if manifest.get("document").and_then(Value::as_str) != Some(digest(&reader.document(16 << 20)?).as_str()) {
        return Err(invalid("stale preview"));
    }
    let data = reader.limited_part("cache/preview.bin", MAX_PREVIEW_BYTES)?;
    if manifest.get("payload").and_then(Value::as_str) != Some(digest(&data).as_str()) {
        return Err(invalid("damaged preview"));
    }
    let entries =
        manifest.get("meshes").and_then(Value::as_array).filter(|m| !m.is_empty() && m.len() <= 1024).ok_or_else(|| invalid("preview bodies"))?;
    let mut preview = DesignPreview::default();
    let mut end = 0usize;
    let mut triangles = 0usize;
    for entry in entries {
        let offset = entry.get("offset").and_then(Value::as_u64).and_then(|n| usize::try_from(n).ok()).ok_or_else(|| invalid("preview offset"))?;
        let length = entry.get("length").and_then(Value::as_u64).and_then(|n| usize::try_from(n).ok()).ok_or_else(|| invalid("preview length"))?;
        if offset != end {
            return Err(invalid("overlapping preview bodies"));
        }
        end = end.checked_add(length).ok_or_else(|| invalid("preview overflow"))?;
        let mut mesh = MeshData { name: String::new(), positions: Vec::new().into(), triangles: Vec::new().into(), color: None };
        decode_mesh(data.get(offset..end).ok_or_else(|| invalid("truncated preview"))?, &mut mesh)?;
        triangles = triangles.saturating_add(mesh.triangles.len() / 3);
        if triangles > MAX_PREVIEW_TRIANGLES {
            return Err(invalid("preview triangles"));
        }
        mesh.name = entry.get("name").and_then(Value::as_str).filter(|n| n.len() <= 1024).ok_or_else(|| invalid("preview name"))?.into();
        if !entry.get("color").is_none_or(Value::is_null) {
            let color: [f32; 3] = serde_json::from_value(entry.get("color").cloned().unwrap_or(Value::Null)).map_err(invalid)?;
            if color.iter().any(|v| !v.is_finite() || !(0.0..=1.0).contains(v)) {
                return Err(invalid("preview color"));
            }
            mesh.color = Some(color);
        }
        preview.meshes.push(mesh);
    }
    if end != data.len() {
        return Err(invalid("trailing preview data"));
    }
    Ok(preview)
}

/// A bounded container reader over a seekable source.
///
/// Opening inspects the directory, not the source blobs. `preview` reads only the
/// optional display entries and document metadata; `read_document` then resolves
/// and validates authoritative blobs against the same retained metadata snapshot.
/// Concurrent source changes either retain that document or fail validation.
/// This type does not turn untrusted cached data into editable geometry.
pub struct DesignContainerReader<R: Read + Seek> {
    reader: PackageReader<R>,
}

impl<R: Read + Seek> DesignContainerReader<R> {
    /// Open a container (not plain JSON), enforcing the same budgets as byte IO.
    pub fn new(source: R) -> Result<Self> {
        Ok(Self { reader: PackageReader::new(source)? })
    }

    /// Optional display-only data. Corruption or incompatibility is a cache miss.
    pub fn preview(&mut self) -> Option<DesignPreview> {
        // An explicitly saved mesh avoids tessellation. Native cache is the default
        // fallback, generating a bounded provisional mesh without source imports.
        read_preview(&mut self.reader).ok().or_else(|| {
            let document = self.reader.document(16 << 20).ok()?;
            native_cache::geometry_preview(&document, &self.geometry()?).ok()
        })
    }

    /// Portable PNG picture only; no source blobs or geometry are evaluated.
    pub fn thumbnail(&mut self) -> Option<Vec<u8>> {
        thumbnail::read(&mut self.reader).ok()
    }

    /// The saved model, when this build wrote it for exactly this document.
    pub fn geometry(&mut self) -> Option<solidcraft_doc::SavedModel> {
        let document = self.reader.document(16 << 20).ok()?;
        native_cache::read_geometry_with(&mut self.reader, &document).ok()
    }

    /// Consume the reader, resolving every required blob before returning a document.
    pub fn read_document(mut self) -> Result<Document> {
        read_document(&mut self.reader)
    }

    /// [`Self::geometry`] when [`Self::read_document`] follows: it validates the document.
    fn geometry_before_document(&mut self) -> Option<solidcraft_doc::SavedModel> {
        let document = self.reader.document(16 << 20).ok()?;
        native_cache::read_geometry_before_document(&mut self.reader, &document).ok()
    }
}

pub(super) fn read(bytes: &[u8]) -> Result<Document> {
    DesignContainerReader::new(Cursor::new(bytes))?.read_document()
}

/// Open from disk (or browser VFS), publishing only provisional display before
/// authoritative blob hydration, with the model the file keeps when this build saved it for
/// exactly this document ([`solidcraft_doc::Model::adopt`] takes it as the evaluation). Desktop
/// keeps one seekable file handle instead of buffering the complete archive. JSON continues
/// through the normal migrations.
pub fn read_design_file(path: &str, notify: Option<impl FnOnce(DesignPreview)>) -> Result<(Document, Option<solidcraft_doc::SavedModel>)> {
    let mut source = open_regular_file(path)?;
    if is_container_source(&mut source)? {
        let mut reader = DesignContainerReader::new(source)?;
        // A design with its model opens without a picture first: the model is the session.
        let model = reader.geometry_before_document();
        if let Some(notify) = notify
            && model.is_none()
            && let Ok(preview) = read_preview(&mut reader.reader)
        {
            notify(preview);
        }
        Ok((reader.read_document()?, model))
    } else {
        let mut bytes = Vec::new();
        source.take((max_design() as u64).saturating_add(1)).read_to_end(&mut bytes).map_err(invalid)?;
        Ok((crate::read_design(&bytes)?, None))
    }
}

#[cfg(not(target_arch = "wasm32"))]
type FileSource = std::fs::File;
#[cfg(target_arch = "wasm32")]
type FileSource = Cursor<Vec<u8>>;

/// A file to read: inside the folders this process may use, a regular file, and at most
/// the largest design file allowed (`limits`), read from its start.
fn open_regular_file(path: &str) -> Result<FileSource> {
    #[cfg(not(target_arch = "wasm32"))]
    let mut source = {
        crate::vfs::check(path).map_err(invalid)?;
        let metadata = std::fs::metadata(path).map_err(invalid)?;
        if !metadata.is_file() {
            return Err(invalid("not a regular file"));
        }
        crate::limits::check(crate::limits::Kind::Design, metadata.len()).map_err(invalid)?;
        let file = std::fs::File::open(path).map_err(invalid)?;
        if !file.metadata().map_err(invalid)?.is_file() {
            return Err(invalid("not a regular file"));
        }
        file
    };
    #[cfg(target_arch = "wasm32")]
    let mut source = Cursor::new(crate::vfs::read(path, max_design()).map_err(invalid)?);
    let size = source.seek(SeekFrom::End(0)).map_err(invalid)?;
    crate::limits::check(crate::limits::Kind::Design, size).map_err(invalid)?;
    source.seek(SeekFrom::Start(0)).map_err(invalid)?;
    Ok(source)
}

/// Whether a source starts as a container does, leaving it at its start.
fn is_container_source(source: &mut FileSource) -> Result<bool> {
    let mut prefix = [0u8; 4];
    let size = source.seek(SeekFrom::End(0)).map_err(invalid)?;
    source.seek(SeekFrom::Start(0)).map_err(invalid)?;
    if size >= 4 {
        source.read_exact(&mut prefix).map_err(invalid)?;
    }
    source.seek(SeekFrom::Start(0)).map_err(invalid)?;
    Ok(is_design_container(&prefix))
}

fn read_document<R: Read + Seek>(reader: &mut PackageReader<R>) -> Result<Document> {
    if drawing::container_kind(reader)? == FileKind::Drawing {
        return Err(IoError::Invalid("this file is a SolidCraft drawing, not a design: open it as a drawing".into()));
    }
    let bytes = reader.document(max_design())?;
    check_hdr_manifest(&bytes)?;
    let mut metadata: Value = serde_json::from_slice(&bytes).map_err(invalid)?;
    // HDR's typed JSON representation is deliberately strict: no empty placeholders or
    // container-only blob fields may reach the document deserializer.
    let hydrated_hdr = hydrate_hdr_resources(&mut metadata, reader)?;
    let hydrated_text;
    let text = if hydrated_hdr {
        hydrated_text = serde_json::to_string(&metadata).map_err(invalid)?;
        &hydrated_text
    } else {
        std::str::from_utf8(&bytes).map_err(invalid)?
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
        canvas.data = solidcraft_doc::canvas::base64_encode(bytes);
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

/// Admit the resource map before Value allocation or duplicate-key coalescing. Encoded
/// sources in a container are mandatory blobs, never arbitrary inline base64 strings.
fn check_hdr_manifest(bytes: &[u8]) -> Result<()> {
    use serde::{
        Deserialize,
        de::{MapAccess, Visitor},
    };
    use solidcraft_doc::render_resources::{HdrId, MAX_HDR_RESOURCES};
    #[derive(Default, Deserialize)]
    struct Render {
        #[serde(default, deserialize_with = "resources")]
        resources: (),
    }
    #[derive(Deserialize)]
    struct Metadata {
        #[serde(default)]
        render: Render,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Entry {
        #[serde(deserialize_with = "kind")]
        kind: (),
        #[serde(deserialize_with = "data")]
        data: (),
        #[serde(deserialize_with = "blob")]
        hdr_blob: (),
    }
    fn marker<'de, D: serde::Deserializer<'de>>(d: D, expected: &'static str) -> std::result::Result<(), D::Error> {
        let raw = <&serde_json::value::RawValue>::deserialize(d)?;
        let maximum = match expected {
            "radiance_hdr" => 12,
            "empty data" => 0,
            _ => 74,
        };
        if raw.get().len() > maximum * 6 + 2 {
            return Err(serde::de::Error::custom("HDR metadata marker exceeds its budget"));
        }
        let value: String = serde_json::from_str(raw.get()).map_err(serde::de::Error::custom)?;
        let valid = match expected {
            "radiance_hdr" => value == "radiance_hdr",
            "empty data" => value.is_empty(),
            _ => value
                .strip_prefix("blobs/")
                .and_then(|v| v.strip_suffix(".hdr"))
                .is_some_and(|v| v.len() == 64 && v.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))),
        };
        if valid { Ok(()) } else { Err(serde::de::Error::custom(format!("HDR metadata requires {expected}"))) }
    }
    fn kind<'de, D: serde::Deserializer<'de>>(d: D) -> std::result::Result<(), D::Error> {
        marker(d, "radiance_hdr")
    }
    fn data<'de, D: serde::Deserializer<'de>>(d: D) -> std::result::Result<(), D::Error> {
        marker(d, "empty data")
    }
    fn blob<'de, D: serde::Deserializer<'de>>(d: D) -> std::result::Result<(), D::Error> {
        marker(d, "an HDR blob reference")
    }
    fn resources<'de, D: serde::Deserializer<'de>>(d: D) -> std::result::Result<(), D::Error> {
        struct Resources;
        impl<'de> Visitor<'de> for Resources {
            type Value = ();
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("unique bounded HDR blob references")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> std::result::Result<(), A::Error> {
                let mut keys = BTreeSet::new();
                while let Some(id) = map.next_key::<HdrId>()? {
                    if keys.len() == MAX_HDR_RESOURCES || !keys.insert(id) {
                        return Err(serde::de::Error::custom("too many or duplicate embedded HDR resources"));
                    }
                    let Entry { kind: (), data: (), hdr_blob: () } = map.next_value()?;
                }
                Ok(())
            }
        }
        d.deserialize_map(Resources)
    }
    let Metadata { render: Render { resources: () } } = serde_json::from_slice(bytes).map_err(invalid)?;
    Ok(())
}

fn hydrate_hdr_resources<R: Read + Seek>(metadata: &mut Value, reader: &mut PackageReader<R>) -> Result<bool> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use solidcraft_geom::Vec2;

    struct CountingReader {
        source: Cursor<Vec<u8>>,
        count: std::rc::Rc<std::cell::Cell<usize>>,
    }

    impl Read for CountingReader {
        fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
            let count = self.source.read(bytes)?;
            self.count.set(self.count.get() + count);
            Ok(count)
        }
    }

    impl Seek for CountingReader {
        fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
            self.source.seek(position)
        }
    }

    #[test]
    fn seekable_reader_rejects_oversized_source_before_reading() {
        struct Oversized;
        impl Read for Oversized {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                panic!("oversized input must be rejected before any allocation or read");
            }
        }
        impl Seek for Oversized {
            fn seek(&mut self, _: SeekFrom) -> std::io::Result<u64> {
                Ok(crate::MAX_DESIGN_BYTES as u64 + 1)
            }
        }
        assert!(DesignContainerReader::new(Oversized).is_err());
    }

    #[test]
    fn seekable_preview_defers_large_source_blob_until_document_read() {
        let mut doc = Document::new("Deferred source");
        let step = noise(2_600_000);
        doc.add_feature(FeatureKind::Import { file: "large.step".into(), step: step.clone().into(), components: Vec::new(), skip: Vec::new() }, None)
            .unwrap();
        let preview = DesignPreview { meshes: vec![mesh()] };
        let bytes = write_design_container_with_preview(&doc, DesignCompression::Fast, Some(&preview)).unwrap();
        let count = std::rc::Rc::new(std::cell::Cell::new(0));
        let source = CountingReader { source: Cursor::new(bytes), count: count.clone() };
        let mut reader = DesignContainerReader::new(source).unwrap();
        assert_eq!(reader.preview().unwrap().meshes[0].positions, preview.meshes[0].positions);
        // ZIP directory scans may touch a bounded suffix of a source blob. They
        // must not load its multi-megabyte payload to obtain the saved picture.
        assert!(count.get() < 256 * 1024, "read {} bytes before display", count.get());
        let restored = reader.read_document().unwrap();
        assert_eq!(restored.to_json(), doc.to_json());
        assert!(count.get() >= step.len() / 4);
    }

    #[test]
    fn seekable_reader_validates_required_blobs_after_preview() {
        let doc = document();
        let preview = DesignPreview { meshes: vec![mesh()] };
        let mut bytes = write_design_container_with_preview(&doc, DesignCompression::Fast, Some(&preview)).unwrap();
        // The first part written is the (compressed) STEP source: damage its data.
        let name = u16::from_le_bytes([bytes[26], bytes[27]]) as usize;
        let extra = u16::from_le_bytes([bytes[28], bytes[29]]) as usize;
        assert!(bytes[30..30 + name].ends_with(b".step"));
        bytes[30 + name + extra + 2] ^= 0x55;
        let mut reader = DesignContainerReader::new(Cursor::new(bytes)).unwrap();
        assert!(reader.preview().is_some());
        assert!(reader.read_document().is_err());
    }

    #[test]
    fn seekable_reader_without_preview_preserves_authoritative_document() {
        let doc = document();
        let bytes = write_design_container(&doc).unwrap();
        let mut source = Cursor::new(bytes);
        source.set_position(17); // Constructor starts from the whole source, not its cursor.
        let mut reader = DesignContainerReader::new(source).unwrap();
        assert!(reader.preview().is_none());
        assert_eq!(reader.read_document().unwrap().to_json(), doc.to_json());
        assert!(DesignContainerReader::new(Cursor::new(vec![0u8; 1])).is_err());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn file_reader_refuses_nonfiles_before_notifying_preview() {
        let directory = std::env::temp_dir();
        let result = read_design_file(directory.to_str().unwrap(), Some(|_| panic!("non-file supplied a preview"))).map(|(doc, _)| doc);
        assert!(result.is_err_and(|e| e.to_string().contains("not a regular file")));
    }

    /// A saved model of one state holding `bodies` (no document evaluated it: enough for the
    /// container, which stores whatever model it is given).
    fn saved_model(bodies: Vec<solidcraft_kernel::Body>) -> solidcraft_doc::SavedModel {
        let records: Vec<Value> = (0..bodies.len())
            .map(|i| json!({"name": format!("Body{i}"), "feature": 1, "component": 0, "key": "0000000000000001", "names": 0, "body": i}))
            .collect();
        let state = json!({
            "bodies": [[0, bodies.len()]], "sketches": [], "body_counter": 7, "sheets": [], "construct": [], "threads": [],
            "sketch_key_set": 0, "blend_edges": {}, "fitted": {}, "tapered": {},
        });
        solidcraft_doc::SavedModel {
            records: json!({
                "features": [{"id": 1, "fingerprint": "0000000000000000", "ms": 0.0, "skipped": false, "state": 0}],
                "states": [state], "model_bodies": records, "names": [{"faces": [], "info": []}], "sketches": [], "sheets": [], "sketch_key_sets": [{"set": [], "drop": []}],
            }),
            bodies,
        }
    }

    fn native_geometry() -> solidcraft_doc::SavedModel {
        saved_model(vec![solidcraft_kernel::box_solid(solidcraft_geom::Vec3::ZERO, solidcraft_geom::Vec3::new(10., 20., 30.)).unwrap()])
    }

    #[test]
    fn a_build_of_unknown_source_never_trusts_a_cache_outside_tests() {
        assert!(!native_cache::source_trusted("unknown", false));
        assert!(native_cache::source_trusted("unknown", true));
        assert!(native_cache::source_trusted("0123abc", false));
    }

    /// The model cache is keyed by the source of the crates a model is computed from (#7168):
    /// each one `doc` is built from must be in it, or an update that changed one would open
    /// designs with the model the code before it computed.
    #[test]
    fn the_cache_key_covers_every_crate_the_model_is_computed_from() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let read = |p: std::path::PathBuf| std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
        let workspace = read(root.join("Cargo.toml"));
        // `solidcraft-geom = { path = "crates/geom" }`: a workspace crate's folder.
        let folder = |name: &str| {
            let prefix = format!("{name} = {{ path = \"crates/");
            workspace.lines().find_map(|l| l.strip_prefix(prefix.as_str())?.split('"').next().map(str::to_string))
        };
        let keyed: Vec<&str> = env!("SOLIDCRAFT_EVALUATION_CRATES").split(',').collect();
        let (mut todo, mut seen) = (vec!["doc".to_string()], BTreeSet::new());
        while let Some(krate) = todo.pop() {
            if !seen.insert(krate.clone()) {
                continue;
            }
            assert!(keyed.contains(&krate.as_str()), "crates/{krate} computes models: add it to EVALUATION_CRATES in crates/io/build.rs");
            let mut built_with = false;
            for line in read(root.join("crates").join(&krate).join("Cargo.toml")).lines() {
                if line.starts_with('[') {
                    built_with = line.ends_with("dependencies]") && !line.contains("dev-dependencies");
                } else if built_with && let Some(dir) = line.split(" = ").next().filter(|n| n.starts_with("solidcraft-")).and_then(folder) {
                    todo.push(dir);
                }
            }
        }
        assert_eq!(seen.len(), keyed.len(), "{seen:?} computed from, {keyed:?} keyed");
    }

    #[test]
    fn native_cache_round_trip_and_preview_defer_original_sources() {
        let mut doc = Document::new("Native cache");
        let step = noise(2_600_000);
        doc.add_feature(FeatureKind::Import { file: "large.step".into(), step: step.clone().into(), components: Vec::new(), skip: Vec::new() }, None)
            .unwrap();
        let bodies = native_geometry();
        let bytes = write_design_container_with_cache(&doc, DesignCompression::Fast, None, Some(&bodies)).unwrap();
        let entries = parts(&bytes);
        assert!(entries.contains_key("cache/bodies.bin"));
        assert!(!entries.contains_key("cache/preview.bin"));
        assert_eq!(read_design_geometry(&bytes).unwrap().bodies[0].body.face_count(), bodies.bodies[0].face_count());
        assert_eq!(bytes, write_design_container_with_cache(&doc, DesignCompression::Fast, None, Some(&bodies)).unwrap());
        let count = std::rc::Rc::new(std::cell::Cell::new(0));
        let mut reader = DesignContainerReader::new(CountingReader { source: Cursor::new(bytes), count: count.clone() }).unwrap();
        assert!(!reader.preview().unwrap().meshes.is_empty());
        assert!(count.get() < 256 * 1024, "read {} bytes before native display", count.get());
        assert_eq!(reader.read_document().unwrap().to_json(), doc.to_json());
        assert!(count.get() >= step.len() / 4);
    }

    #[test]
    fn model_cache_stores_repeated_parts_once_and_keeps_placed_copies() {
        use solidcraft_geom::Vec3;
        let part = solidcraft_kernel::box_solid(Vec3::ZERO, Vec3::new(10., 20., 30.)).unwrap();
        let share = solidcraft_kernel::DisplayShare::new(part.clone());
        let mut bodies = vec![part.clone(), part.clone()];
        for x in [50., 90.] {
            let moved = solidcraft_kernel::transform(&part, Vec3::new(x, 0., 0.), Vec3::ZERO, Vec3::Z, 0.).unwrap();
            let t = [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.], [x, 0., 0., 1.]];
            bodies.push(moved.with_display_share(&share, t));
        }
        let model = saved_model(bodies);
        let doc = document();
        let bytes = write_design_container_with_cache(&doc, DesignCompression::Fast, None, Some(&model)).unwrap();
        let manifest: Value = serde_json::from_slice(&parts(&bytes)["cache/model.json"]).unwrap();
        assert_eq!(manifest["slices"].as_array().unwrap().len(), 3, "the part and each placed copy, once each");
        let read = DesignContainerReader::new(Cursor::new(&bytes)).unwrap().geometry().unwrap();
        assert_eq!(read.records, model.records);
        assert_eq!(read_design_geometry(&bytes).unwrap().body_counter, 7);
        let copies: Vec<_> = read.bodies[2..].iter().map(|b| b.display_share().unwrap()).collect();
        assert_eq!(copies[0].0.id(), copies[1].0.id(), "copies show one display again");
        assert_eq!(copies[1].1[3][0], 90.);
        assert!(read.bodies[0].display_share().is_none());
        for (a, b) in model.bodies.iter().zip(&read.bodies) {
            assert_eq!(solidcraft_kernel::encode_native_body(a).unwrap(), solidcraft_kernel::encode_native_body(b).unwrap());
        }
    }

    /// The model cache's manifest, written straight from the model's records (not copied into a
    /// JSON tree first), is byte for byte what writing it as a tree wrote: the same keys in the
    /// same (sorted) order, the same numbers.
    #[test]
    fn the_model_cache_manifest_is_what_a_json_tree_writes() {
        use solidcraft_geom::Vec3;
        let part = solidcraft_kernel::box_solid(Vec3::ZERO, Vec3::new(10., 20., 30.)).unwrap();
        let share = solidcraft_kernel::DisplayShare::new(part.clone());
        let t = [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.], [50.5, -0.25, 1e-7, 1.]];
        let moved = solidcraft_kernel::transform(&part, Vec3::new(50.5, -0.25, 1e-7), Vec3::ZERO, Vec3::Z, 0.).unwrap();
        let model = saved_model(vec![part.clone(), moved.with_display_share(&share, t)]);
        let bytes = write_design_container_with_cache(&document(), DesignCompression::Fast, None, Some(&model)).unwrap();
        let manifest = &parts(&bytes)["cache/model.json"];
        let tree: Value = serde_json::from_slice(manifest).unwrap();
        assert_eq!(serde_json::to_vec(&tree).unwrap(), *manifest);
        assert_eq!(tree["model"], model.records);
        assert_eq!(tree.as_object().unwrap().len(), 11, "{:?}", tree.as_object().unwrap().keys().collect::<Vec<_>>());
    }

    /// #9170: a mesh body (an imported STL) is kept in the model cache with the rest of the
    /// model, as its own triangles, colour and paint. One mesh body left the whole cache out, so
    /// opening the design evaluated every feature again.
    #[test]
    fn model_cache_keeps_mesh_bodies_with_the_rest_of_the_model() {
        use solidcraft_geom::Vec3;
        let corners = [Vec3::ZERO, Vec3::new(10., 0., 0.), Vec3::new(0., 10., 0.), Vec3::new(0., 0., 10.)];
        let tetra = solidcraft_kernel::mesh_body(&corners, &[[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]])
            .unwrap()
            .with_color(Some([0.25, 0.5, 0.75]))
            .with_paint(Some(solidcraft_kernel::Paint { opacity: 0.5, faces: Vec::new() }));
        let block = solidcraft_kernel::box_solid(Vec3::new(40., 0., 0.), Vec3::new(60., 20., 20.)).unwrap();
        let model = saved_model(vec![block, tetra.clone()]);
        let doc = document();
        let bytes = write_design_container_with_cache(&doc, DesignCompression::Fast, None, Some(&model)).unwrap();
        let entries = parts(&bytes);
        let manifest: Value = serde_json::from_slice(&entries["cache/model.json"]).expect("the cache is written");
        assert_eq!(manifest["slices"].as_array().unwrap().len(), 2);
        let read = DesignContainerReader::new(Cursor::new(&bytes)).unwrap().geometry().unwrap();
        assert_eq!(read.records, model.records);
        assert_eq!(solidcraft_kernel::encode_native_body(&read.bodies[0]).unwrap(), solidcraft_kernel::encode_native_body(&model.bodies[0]).unwrap());
        let mesh = &read.bodies[1];
        assert_eq!(mesh.triangle_mesh(), tetra.triangle_mesh());
        assert_eq!((mesh.color(), mesh.paint()), (tetra.color(), tetra.paint()));
        // The picture shown while opening draws the mesh body with its own triangles.
        let preview = read_design_preview(&bytes).unwrap();
        assert_eq!(preview.meshes.len(), 2);
        let (positions, triangles) = tetra.triangle_mesh().unwrap();
        assert_eq!(&*preview.meshes[1].positions, &positions.iter().flat_map(|p| [p.x, p.y, p.z]).collect::<Vec<_>>()[..]);
        assert_eq!(&*preview.meshes[1].triangles, &triangles.iter().flatten().copied().collect::<Vec<_>>()[..]);
        assert_eq!(preview.meshes[1].color, tetra.color());

        // A changed or cut-short mesh slice (signed again, so it reaches the slice's reader) is a
        // cache miss or another mesh, never a crash.
        let range = |i: usize| manifest["slices"][1][i].as_u64().unwrap() as usize;
        let (start, length) = (range(0), range(1));
        let payload = entries["cache/bodies.bin"].clone();
        assert_eq!(payload.len(), start + length, "the mesh slice is the last");
        let with = |payload: &[u8], length: usize| {
            let mut entries = entries.clone();
            let mut manifest = manifest.clone();
            manifest["payload"] = json!(digest(payload));
            manifest["slices"][1][1] = json!(length);
            entries.insert("cache/model.json".into(), serde_json::to_vec(&manifest).unwrap());
            entries.insert("cache/bodies.bin".into(), payload.to_vec());
            package(&entries)
        };
        assert!(read_design_geometry(&with(&payload, length)).is_some(), "signed again as it was, it reads");
        for n in (start..start + length).step_by(5) {
            let mut changed = payload.clone();
            changed[n] ^= 0xff;
            let changed = with(&changed, length);
            assert!(std::panic::catch_unwind(|| (read_design_geometry(&changed), read_design_preview(&changed))).is_ok(), "byte {n}");
            assert!(read_design_geometry(&with(&payload[..n], n - start)).is_none(), "cut short at {n}");
        }
    }

    /// A model cache that does not fit the design file is left out: the save succeeds, and the
    /// design reads back as saved.
    #[test]
    fn a_model_cache_too_big_for_the_file_is_left_out_not_a_failed_save() {
        let doc = document();
        let bytes = write_container(&doc, DesignCompression::Fast, None, Some(&native_geometry()), None, 70_000).unwrap();
        assert!(!parts(&bytes).contains_key("cache/model.json") && !parts(&bytes).contains_key("cache/bodies.bin"));
        assert!(read_design_geometry(&bytes).is_none());
        assert_eq!(crate::read_design(&bytes).unwrap().to_json(), doc.to_json());
        // With room for it, the same model is kept.
        let bytes = write_container(&doc, DesignCompression::Fast, None, Some(&native_geometry()), None, crate::MAX_DESIGN_BYTES).unwrap();
        assert!(read_design_geometry(&bytes).is_some());
    }

    /// Opening a design with its model validates its document once, where the document is read
    /// (`read_document`); reading the model first validated it a second time, a whole parse and
    /// validation of a large design. A broken document still fails the open, as it did.
    #[test]
    fn opening_with_a_model_cache_validates_the_document_once() {
        let validated = || native_cache::VALIDATED.with(std::cell::Cell::get);
        let doc = document();
        let bytes = write_design_container_with_cache(&doc, DesignCompression::Fast, None, Some(&native_geometry())).unwrap();
        let before = validated();
        let alone = DesignContainerReader::new(Cursor::new(&bytes)).unwrap().geometry().unwrap();
        assert_eq!(validated(), before + 1, "the model alone checks its document");
        let mut reader = DesignContainerReader::new(Cursor::new(&bytes)).unwrap();
        let model = reader.geometry_before_document().unwrap();
        assert_eq!(validated(), before + 1, "opening leaves it to read_document");
        assert_eq!(reader.read_document().unwrap().to_json(), doc.to_json());
        assert_eq!(model.records, alone.records);
        assert_eq!(model.bodies.len(), alone.bodies.len());

        // A broken document whose cache is signed for it: the open fails with read_document's
        // error, as when the model was read with the document validated (and so missed).
        let entries = parts(&bytes);
        let mut manifest: Value = serde_json::from_slice(&entries["cache/model.json"]).unwrap();
        let mut metadata: Value = serde_json::from_slice(&entries["document.json"]).unwrap();
        metadata["features"] = json!(5);
        let truncated = entries["document.json"][..entries["document.json"].len() / 2].to_vec();
        for broken in [serde_json::to_vec(&metadata).unwrap(), truncated] {
            let mut entries = entries.clone();
            manifest["document"] = json!(digest(&broken));
            entries.insert("cache/model.json".into(), serde_json::to_vec(&manifest).unwrap());
            entries.insert("document.json".into(), broken);
            let bytes = package(&entries);
            assert!(DesignContainerReader::new(Cursor::new(&bytes)).unwrap().geometry().is_none());
            let expected = read(&bytes).unwrap_err().to_string();
            let mut reader = DesignContainerReader::new(Cursor::new(&bytes)).unwrap();
            let _ = reader.geometry_before_document();
            assert_eq!(reader.read_document().unwrap_err().to_string(), expected);
        }
    }

    /// A saved model of four features (5, 50, 20 and 1 ms) whose states each hold the eight
    /// bodies all of them share and eight of their own (a body each feature replaces). No
    /// document evaluated it.
    fn timeline_model() -> solidcraft_doc::SavedModel {
        let bodies: Vec<_> = (0..40)
            .map(|i| solidcraft_kernel::box_solid(solidcraft_geom::Vec3::ZERO, solidcraft_geom::Vec3::new(10. + f64::from(i), 20., 30.)).unwrap())
            .collect();
        let records: Vec<Value> = (0..40)
            .map(|i| json!({"name": format!("Body{i}"), "feature": 1 + i / 8, "component": 0, "key": "0000000000000001", "names": 0, "body": i}))
            .collect();
        // As runs of consecutive bodies, which the first state's two groups make one of.
        let states: Vec<Value> = (0..4)
            .map(|i| {
                let bodies = if i == 0 { json!([[0, 16]]) } else { json!([[0, 8], [8 * (i + 1), 8]]) };
                json!({
                    "bodies": bodies, "sketches": [], "body_counter": 7, "sheets": [], "construct": [], "threads": [],
                    "sketch_key_set": 0, "blend_edges": {}, "fitted": {}, "tapered": {},
                })
            })
            .collect();
        let features: Vec<Value> = [5., 50., 20., 1.]
            .iter()
            .enumerate()
            .map(|(i, ms)| json!({"id": i + 1, "fingerprint": "0000000000000000", "ms": ms, "skipped": false, "state": i}))
            .collect();
        solidcraft_doc::SavedModel {
            records: json!({"features": features, "states": states, "model_bodies": records, "names": [{"faces": [], "info": []}], "sketches": [], "sheets": [], "sketch_key_sets": [{"set": [], "drop": []}]}),
            bodies,
        }
    }

    /// Which features keep their state in `model`.
    fn kept_states(model: &Value) -> Vec<bool> {
        model["features"].as_array().unwrap().iter().map(|f| f.get("state").is_some()).collect()
    }

    /// Every body a megabyte, so the records (a few kilobytes) fit in the room left over.
    const BODY: usize = 1 << 20;

    /// A thinned model keeps the final state, then the slowest features' states while the bodies
    /// they add fit, each body counted once however many states hold it (#3220).
    #[test]
    fn a_thinned_model_keeps_the_final_state_then_the_slowest_that_fit() {
        let model = timeline_model();
        let pieces: Vec<Vec<(usize, usize)>> = (0..40).map(|i| vec![(i, BODY)]).collect();
        let thinned = |bodies: usize| {
            let budget = bodies * BODY + BODY / 2;
            model.thinned(&pieces, budget).map(|(m, kept, counted)| (kept_states(&m.records), kept, m, counted <= budget)).unwrap()
        };
        let (states, kept, all, within) = thinned(40);
        assert_eq!((states, kept.len(), within), (vec![true; 4], 40, true));
        assert_eq!(all.records, model.records);
        // The final state and the slowest other (50 ms): the shared bodies count once, else
        // these two states would need 32 bodies' room.
        for bodies in [24, 31] {
            let (states, kept, two, _) = thinned(bodies);
            assert_eq!(states, vec![false, true, false, true], "room for {bodies} bodies");
            assert_eq!(kept, (0..8).chain(16..24).chain(32..40).collect::<Vec<_>>());
            assert_eq!(two.records["model_bodies"].as_array().unwrap().len(), 24);
            let last = two.final_state().unwrap();
            assert_eq!(
                last.bodies.iter().map(|b| b.name.as_str()).collect::<Vec<_>>(),
                (0..8).chain(32..40).map(|i| format!("Body{i}")).collect::<Vec<_>>()
            );
            assert_eq!(last.body_counter, 7);
        }
        assert_eq!(thinned(32).0, vec![false, true, true, true]);
        // The final state only: within the budget, then kept over it (nothing smaller is left).
        for (bodies, within) in [(16, true), (23, true), (15, false)] {
            let (states, kept, one, fits) = thinned(bodies);
            assert_eq!((states, fits), (vec![false, false, false, true], within), "room for {bodies} bodies");
            assert_eq!(kept, (0..8).chain(32..40).collect::<Vec<_>>());
            assert_eq!(
                one.records["states"],
                json!([{
                    "bodies": [[0, 16]], "sketches": [], "body_counter": 7, "sheets": [], "construct": [], "threads": [],
                    "sketch_key_set": 0, "blend_edges": {}, "fitted": {}, "tapered": {},
                }])
            );
        }
    }

    /// A model cache too big for the file keeps fewer states rather than none (#3220): the final
    /// state and as many of the slowest others as fit, then the final state only, and nothing
    /// only when not even that fits.
    #[test]
    fn a_model_cache_too_big_for_the_file_keeps_fewer_states() {
        let model = timeline_model();
        let document = b"{}";
        let states = |space| {
            let (manifest, payload) = native_cache::encode_model(&model, document, space).ok()?;
            assert!(manifest.len() + payload.len() <= space);
            Some(kept_states(&serde_json::from_slice::<Value>(&manifest).unwrap()["model"]))
        };
        // The cache of the states room for so many bodies picks, at its own size.
        let pieces: Vec<Vec<(usize, usize)>> = (0..40).map(|i| vec![(i, BODY)]).collect();
        let cache = |bodies: usize| {
            let (thin, _, _) = model.thinned(&pieces, bodies * BODY + BODY / 2).unwrap();
            let (manifest, payload) = native_cache::encode_model(&thin, document, usize::MAX).unwrap();
            (manifest.len(), payload.len())
        };
        let encoded = |i: usize| solidcraft_kernel::encode_native_body(&model.bodies[i]).unwrap().len();
        // What a state's own bodies add: more than the records of the caches below, so the sizes
        // below pick one set of states each.
        let own = (8..40).step_by(8).map(|s| (s..s + 8).map(encoded).sum::<usize>()).min().unwrap();
        let (all, two, one) = (cache(40), cache(24), cache(16));
        assert!(own > two.0, "a state's bodies ({own} bytes) outweigh the records ({} bytes)", two.0);
        assert_eq!(states(all.0 + all.1), Some(vec![true; 4]));
        assert_eq!(states(all.0 + all.1 - 1), Some(vec![false, true, true, true]), "the fastest left out first");
        assert_eq!(states(two.0 + two.1), Some(vec![false, true, false, true]));
        assert_eq!(states(one.0 + one.1), Some(vec![false, false, false, true]));
        assert_eq!(states(one.0 + one.1 - 1), None);
    }

    /// A model whose bodies are all one part stores it once: its records, not its bodies, fill the
    /// room. When those of every state do not fit, the final state is still kept if it fits.
    #[test]
    fn a_model_cache_of_records_too_big_for_the_file_keeps_the_final_state() {
        let mut model = timeline_model();
        let part = model.bodies[0].clone();
        model.bodies.iter_mut().for_each(|b| *b = part.clone());
        let document = b"{}";
        let size = |m: &solidcraft_doc::SavedModel| {
            let (manifest, payload) = native_cache::encode_model(m, document, usize::MAX).unwrap();
            (manifest.len() + payload.len(), serde_json::from_slice::<Value>(&manifest).unwrap()["slices"].as_array().unwrap().len())
        };
        let pieces: Vec<Vec<(usize, usize)>> = (0..40).map(|i| vec![(i, BODY)]).collect();
        let (last, _, _) = model.thinned(&pieces, 16 * BODY + BODY / 2).unwrap();
        let ((all, slices), (one, _)) = (size(&model), size(&last));
        assert_eq!(slices, 1, "the part stored once");
        // Room for the final state's cache and a little more, not for the records of all.
        let space = one + 256;
        assert!(all > space);
        let (manifest, _) = native_cache::encode_model(&model, document, space).unwrap();
        assert_eq!(kept_states(&serde_json::from_slice::<Value>(&manifest).unwrap()["model"]), vec![false, false, false, true]);
    }

    #[test]
    fn native_cache_mismatch_is_only_a_cache_miss() {
        let doc = document();
        let bytes = write_design_container_with_cache(&doc, DesignCompression::Fast, None, Some(&native_geometry())).unwrap();
        let original = parts(&bytes);
        for key in ["format", "producer", "evaluation", "document", "payload"] {
            let mut entries = original.clone();
            let mut manifest: Value = serde_json::from_slice(&entries["cache/model.json"]).unwrap();
            manifest[key] = json!("unsupported-or-stale");
            entries.insert("cache/model.json".into(), serde_json::to_vec(&manifest).unwrap());
            let bytes = package(&entries);
            assert!(read_design_geometry(&bytes).is_none());
            assert!(read_design_preview(&bytes).is_none());
            assert_eq!(crate::read_design(&bytes).unwrap().to_json(), doc.to_json());
        }
        // A cache of the format before sketch-key sets (/4) is a miss, and so are records of
        // that shape (each state's keys inline) under the current format: never misread.
        let manifest: Value = serde_json::from_slice(&original["cache/model.json"]).unwrap();
        let mut older = manifest.clone();
        older["format"] = json!("solidcraft-model-cache/4");
        let mut inline = manifest.clone();
        let model = inline["model"].as_object_mut().unwrap();
        model.remove("sketch_key_sets");
        let state = model["states"][0].as_object_mut().unwrap();
        state.remove("sketch_key_set");
        state.insert("sketch_keys".into(), json!([]));
        for manifest in [older, inline] {
            let mut entries = original.clone();
            entries.insert("cache/model.json".into(), serde_json::to_vec(&manifest).unwrap());
            let bytes = package(&entries);
            assert!(read_design_geometry(&bytes).is_none(), "{}", manifest["format"]);
            assert_eq!(crate::read_design(&bytes).unwrap().to_json(), doc.to_json());
        }
        let mut entries = original;
        entries.get_mut("cache/bodies.bin").unwrap()[0] ^= 1;
        let bytes = package(&entries);
        assert!(read_design_geometry(&bytes).is_none());
        assert_eq!(crate::read_design(&bytes).unwrap().to_json(), doc.to_json());
    }

    /// #7168: a design saved by another version of SolidCraft (another version number and commit)
    /// whose model was computed by the same evaluation source opens from its model cache, so an
    /// update that changed only the interface or the commands does not evaluate every design again.
    /// One computed by other evaluation source (a kernel or codec change) is evaluated again.
    #[test]
    fn a_cache_of_another_version_with_the_same_evaluation_source_is_read() {
        let doc = document();
        let geometry = native_geometry();
        let bytes = write_design_container_with_cache(&doc, DesignCompression::Fast, None, Some(&geometry)).unwrap();
        let written = |entries: &BTreeMap<String, Vec<u8>>| serde_json::from_slice::<Value>(&entries["cache/model.json"]).unwrap();
        let original = parts(&bytes);
        let evaluation = written(&original)["evaluation"].as_str().unwrap().to_string();
        assert!(evaluation.ends_with(&format!("/{}-{}", std::env::consts::ARCH, std::env::consts::OS)), "{evaluation}");
        let with = |changes: &[(&str, &str)]| {
            let mut entries = original.clone();
            let mut manifest = written(&entries);
            for (key, value) in changes {
                manifest[*key] = json!(value);
            }
            entries.insert("cache/model.json".into(), serde_json::to_vec(&manifest).unwrap());
            package(&entries)
        };
        let older = with(&[("app", "0.0.1"), ("build", "0123456789abcdef0123456789abcdef01234567/other-platform")]);
        let state = read_design_geometry(&older).expect("another version's cache of the same evaluation source");
        assert_eq!(state.bodies[0].body.face_count(), geometry.bodies[0].face_count());
        assert_eq!(crate::read_design(&older).unwrap().to_json(), doc.to_json());
        // The same version with other evaluation source, or on another platform, evaluates again.
        let other_source = format!("{}/{}", "0".repeat(64), evaluation.rsplit('/').next().unwrap());
        for changed in [other_source.as_str(), &format!("{}/other-platform", evaluation.rsplit_once('/').unwrap().0)] {
            let bytes = with(&[("evaluation", changed)]);
            assert!(read_design_geometry(&bytes).is_none(), "{changed}");
            assert_eq!(crate::read_design(&bytes).unwrap().to_json(), doc.to_json());
        }
        // An older build's cache (no evaluation source recorded) is evaluated again.
        let mut entries = original.clone();
        let mut manifest = written(&entries);
        manifest.as_object_mut().unwrap().remove("evaluation");
        entries.insert("cache/model.json".into(), serde_json::to_vec(&manifest).unwrap());
        assert!(read_design_geometry(&package(&entries)).is_none());
    }

    #[test]
    fn metadata_snapshot_survives_in_place_changes_after_preview() {
        use std::{cell::RefCell, rc::Rc};
        struct Shared {
            bytes: Rc<RefCell<Vec<u8>>>,
            position: u64,
        }
        impl Read for Shared {
            fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
                let bytes = self.bytes.borrow();
                let mut cursor = Cursor::new(bytes.as_slice());
                cursor.set_position(self.position);
                let n = cursor.read(out)?;
                self.position = cursor.position();
                Ok(n)
            }
        }
        impl Seek for Shared {
            fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
                let bytes = self.bytes.borrow();
                let mut cursor = Cursor::new(bytes.as_slice());
                cursor.set_position(self.position);
                self.position = cursor.seek(pos)?;
                Ok(self.position)
            }
        }
        let doc = document();
        let preview = DesignPreview { meshes: vec![mesh()] };
        let bytes = write_design_container_with_preview(&doc, DesignCompression::Fast, Some(&preview)).unwrap();
        let offset = zip::ZipArchive::new(Cursor::new(&bytes)).unwrap().by_name("document.json").unwrap().data_start().unwrap() as usize;
        let bytes = Rc::new(RefCell::new(bytes));
        let mut reader = DesignContainerReader::new(Shared { bytes: bytes.clone(), position: 0 }).unwrap();
        assert!(reader.preview().is_some());
        bytes.borrow_mut()[offset] ^= 0xff;
        assert_eq!(reader.read_document().unwrap().to_json(), doc.to_json());
        assert!(crate::read_design(&bytes.borrow()).is_err());
    }

    #[test]
    fn native_cache_prefixes_and_mutations_never_panic() {
        let bytes = write_design_container_with_cache(&document(), DesignCompression::Fast, None, Some(&native_geometry())).unwrap();
        for n in (0..bytes.len()).step_by((bytes.len() / 128).max(1)) {
            assert!(
                std::panic::catch_unwind(|| {
                    let _ = read_design_preview(&bytes[..n]);
                    let _ = read_design_geometry(&bytes[..n]);
                })
                .is_ok()
            );
            let mut changed = bytes.clone();
            changed[n] ^= 0xff;
            assert!(
                std::panic::catch_unwind(|| {
                    let _ = read_design_preview(&changed);
                    let _ = read_design_geometry(&changed);
                })
                .is_ok()
            );
        }
    }

    fn mesh() -> MeshData {
        MeshData {
            name: "Precise mesh".into(),
            positions: vec![0.123456789012345, -0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0].into(),
            triangles: vec![0, 1, 2].into(),
            color: Some([0.25, 0.5, 0.75]),
        }
    }

    fn document() -> Document {
        let mut doc = Document::new("Payloads");
        for _ in 0..2 {
            doc.add_feature(
                FeatureKind::Import {
                    file: "example.step".into(),
                    step: "ISO-10303-21;\nEND-ISO-10303-21;".into(),
                    components: Vec::new(),
                    skip: Vec::new(),
                },
                None,
            )
            .unwrap();
        }
        let mut other = mesh();
        other.name = "Same triangles, separate appearance".into();
        other.color = None;
        doc.add_feature(FeatureKind::MeshImport { file: "example.obj".into(), meshes: vec![mesh(), other] }, None).unwrap();
        let id = doc.allocate_id().unwrap();
        doc.canvases.push(solidcraft_doc::canvas::Canvas {
            id,
            name: "Reference".into(),
            format: "png".into(),
            data: solidcraft_doc::canvas::base64_encode(b"original image bytes"),
            pixels: [1, 1],
            plane: solidcraft_doc::PlaneRef::Origin { name: "XY".into() },
            center: Vec2::new(0.0, 0.0),
            width: 10.0,
            angle: 0.0,
            opacity: 0.5,
            flip: false,
            visible: true,
            component: 0,
        });
        doc
    }

    fn parts(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        (0..zip.len())
            .map(|i| {
                let mut entry = zip.by_index(i).unwrap();
                let name = entry.name().unwrap().into_owned();
                let mut bytes = Vec::new();
                entry.read_to_end(&mut bytes).unwrap();
                (name, bytes)
            })
            .collect()
    }

    fn package(parts: &BTreeMap<String, Vec<u8>>) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, bytes) in parts {
            zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    #[test]
    fn portable_thumbnail_is_keyed_bounded_and_does_not_read_sources() {
        // IO validates an opaque PNG header/hash; UI tests exercise real PNG decoding.
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend(240u32.to_be_bytes());
        png.extend(150u32.to_be_bytes());
        let mut doc = Document::from_json(&document().to_json()).unwrap();
        if let FeatureKind::Import { step, .. } = &mut doc.features[0].kind {
            *step = "source data".repeat(250_000).into();
        }
        let bytes = write_design_container_with_thumbnail(&doc, DesignCompression::Fast, None, None, Some(&png)).unwrap();
        let count = std::rc::Rc::new(std::cell::Cell::new(0));
        let mut reader = DesignContainerReader::new(CountingReader { source: Cursor::new(bytes.clone()), count: count.clone() }).unwrap();
        assert_eq!(reader.thumbnail(), Some(png.clone()));
        assert!(count.get() < 256 * 1024);
        assert_eq!(reader.read_document().unwrap(), doc);
        let original = parts(&bytes);
        for field in ["format", "app", "document", "payload"] {
            let mut entries = original.clone();
            let mut key: Value = serde_json::from_slice(&entries["cache/thumbnail.json"]).unwrap();
            key[field] = json!("mismatch");
            entries.insert("cache/thumbnail.json".into(), serde_json::to_vec(&key).unwrap());
            let changed = package(&entries);
            assert!(read_design_thumbnail(&changed).is_none());
            assert_eq!(crate::read_design(&changed).unwrap(), doc);
        }
        let mut entries = original.clone();
        let source = entries.keys().find(|k| k.ends_with(".step")).unwrap().clone();
        entries.insert(source, b"corrupt source".to_vec());
        let changed = package(&entries);
        assert_eq!(read_design_thumbnail(&changed), Some(png));
        assert!(crate::read_design(&changed).is_err());
        for n in [0, 1, 8, bytes.len() / 2, bytes.len() - 1] {
            assert!(read_design_thumbnail(&bytes[..n]).is_none());
        }
        for i in (0..bytes.len()).step_by((bytes.len() / 128).max(1)) {
            let mut changed = bytes.clone();
            changed[i] ^= 0xff;
            assert!(std::panic::catch_unwind(|| read_design_thumbnail(&changed)).is_ok());
        }
        for invalid in [vec![], vec![0; 300_000]] {
            let bytes = write_design_container_with_thumbnail(&doc, DesignCompression::Fast, None, None, Some(&invalid)).unwrap();
            assert!(read_design_thumbnail(&bytes).is_none());
            assert_eq!(crate::read_design(&bytes).unwrap(), doc);
        }
    }

    #[test]
    fn optional_preview_is_bounded_keyed_and_never_authoritative() {
        let doc = Document::from_json(&document().to_json()).unwrap();
        let preview = DesignPreview { meshes: vec![mesh()] };
        let bytes = write_design_container_with_preview(&doc, DesignCompression::Fast, Some(&preview)).unwrap();
        let loaded = read_design_preview(&bytes).unwrap();
        assert_eq!(loaded.meshes, preview.meshes);
        assert_eq!(crate::read_design(&bytes).unwrap(), doc);
        let original = parts(&bytes);
        for field in ["format", "build", "document", "payload"] {
            let mut entries = original.clone();
            let mut manifest: Value = serde_json::from_slice(&entries["cache/preview.json"]).unwrap();
            manifest[field] = json!("obsolete");
            entries.insert("cache/preview.json".into(), serde_json::to_vec(&manifest).unwrap());
            let changed = package(&entries);
            assert!(read_design_preview(&changed).is_none(), "{field}");
            assert_eq!(crate::read_design(&changed).unwrap(), doc);
        }
        let mut entries = original.clone();
        let mut metadata: Value = serde_json::from_slice(&entries["document.json"]).unwrap();
        metadata["name"] = json!("Changed document");
        entries.insert("document.json".into(), serde_json::to_vec(&metadata).unwrap());
        assert!(read_design_preview(&package(&entries)).is_none());
        for damaged in [vec![], vec![0; 128], vec![0; MAX_PREVIEW_BYTES + 1]] {
            let mut entries = original.clone();
            entries.insert("cache/preview.bin".into(), damaged);
            let changed = package(&entries);
            assert!(read_design_preview(&changed).is_none());
            assert_eq!(crate::read_design(&changed).unwrap(), doc);
        }
        // Even a matching checksum does not excuse invalid indices or overlapping slices.
        for overlap in [false, true] {
            let mut entries = original.clone();
            let mut manifest: Value = serde_json::from_slice(&entries["cache/preview.json"]).unwrap();
            if overlap {
                manifest["meshes"][0]["offset"] = json!(1);
            } else {
                let data = entries.get_mut("cache/preview.bin").unwrap();
                let end = data.len();
                data[end - 4..].copy_from_slice(&u32::MAX.to_le_bytes());
                manifest["payload"] = json!(digest(data));
            }
            entries.insert("cache/preview.json".into(), serde_json::to_vec(&manifest).unwrap());
            assert!(read_design_preview(&package(&entries)).is_none());
        }
        let mut too_large = mesh();
        too_large.triangles = vec![0; (MAX_PREVIEW_TRIANGLES + 1) * 3].into();
        let bytes = write_design_container_with_preview(&doc, DesignCompression::Fast, Some(&DesignPreview { meshes: vec![too_large] })).unwrap();
        assert!(read_design_preview(&bytes).is_none());
        assert_eq!(crate::read_design(&bytes).unwrap(), doc);
        assert!(read_design_preview(&crate::write_design(&doc)).is_none());
    }

    #[test]
    fn round_trip_deduplicates_and_preserves_binary_precision() {
        let source = Document::from_json(&document().to_json()).unwrap();
        let bytes = write_design_container(&source).unwrap();
        assert!(is_design_container(&bytes));
        let reopened = crate::read_design(&bytes).unwrap();
        assert_eq!(source, reopened);
        let FeatureKind::MeshImport { meshes, .. } = &reopened.features[2].kind else { panic!() };
        assert_eq!(meshes[0].positions[1].to_bits(), (-0.0f64).to_bits());
        let entries = parts(&bytes);
        assert_eq!(entries.keys().filter(|n| n.ends_with(".step")).count(), 1);
        assert_eq!(entries.keys().filter(|n| n.ends_with(".mesh")).count(), 1);
        assert_eq!(entries.keys().filter(|n| n.ends_with(".png")).count(), 1);
        let json: Value = serde_json::from_slice(&entries["document.json"]).unwrap();
        assert_eq!(json["features"][2]["meshes"][0]["positions"], json!([]));
        assert_eq!(crate::read_design(&crate::write_design(&reopened)).unwrap(), source);
        assert_eq!(write_design_container(&source).unwrap(), bytes, "deterministic archive");
        let compact = write_design_container_with_compression(&source, DesignCompression::Compact).unwrap();
        assert_eq!(crate::read_design(&compact).unwrap(), source);
    }

    #[test]
    fn draft_container_marker_remains_readable() {
        let doc = Document::from_json(&document().to_json()).unwrap();
        let mut entries = parts(&write_design_container(&doc).unwrap());
        let marker: Value = serde_json::from_slice(&entries["container.json"]).unwrap();
        assert_eq!(marker["format"], VERSION);
        entries.insert("container.json".into(), serde_json::to_vec(&json!({"format": LEGACY_VERSION})).unwrap());
        assert_eq!(crate::read_design(&package(&entries)).unwrap(), doc);
    }

    #[test]
    fn required_blobs_must_exist_and_match_hashes() {
        let bytes = write_design_container(&document()).unwrap();
        let mut entries = parts(&bytes);
        let name = entries.keys().find(|n| n.ends_with(".step")).unwrap().clone();
        let original = entries.remove(&name).unwrap();
        assert!(crate::read_design(&package(&entries)).unwrap_err().to_string().contains("required part"));
        entries.insert(name.clone(), b"damaged".to_vec());
        assert!(crate::read_design(&package(&entries)).unwrap_err().to_string().contains("hash mismatch"));
        entries.insert(name, original);
        entries.insert("cache/obsolete.bin".into(), b"untrusted optional cache".to_vec());
        assert!(crate::read_design(&package(&entries)).is_ok());
    }

    #[test]
    fn refuses_hostile_mesh_lengths_numbers_indices_and_versions() {
        let valid = encode_mesh(&mesh()).unwrap();
        for offset in [8, 16] {
            let mut bytes = valid.clone();
            bytes[offset..offset + 8].copy_from_slice(&u64::MAX.to_le_bytes());
            assert!(decode_mesh(&bytes, &mut mesh()).is_err());
        }
        let mut bytes = valid.clone();
        bytes[24..32].copy_from_slice(&f64::NAN.to_le_bytes());
        assert!(decode_mesh(&bytes, &mut mesh()).is_err());
        let mut bytes = valid.clone();
        let offset = bytes.len() - 4;
        bytes[offset..].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(decode_mesh(&bytes, &mut mesh()).is_err());
        for length in 0..valid.len() {
            assert!(decode_mesh(&valid[..length], &mut mesh()).is_err());
        }
        let mut bytes = valid;
        bytes[7] = b'2';
        assert!(decode_mesh(&bytes, &mut mesh()).is_err());
    }

    #[test]
    fn hostile_archive_paths_versions_and_truncations_are_errors() {
        let bytes = write_design_container(&document()).unwrap();
        let mut entries = parts(&bytes);
        entries.insert("../escape".into(), vec![]);
        assert!(crate::read_design(&package(&entries)).is_err());
        entries.remove("../escape");
        entries.insert("container.json".into(), br#"{"format":"solidcraft-container/999"}"#.to_vec());
        assert!(crate::read_design(&package(&entries)).unwrap_err().to_string().contains("unsupported container version"));
        for length in (0..bytes.len()).step_by(17) {
            assert!(crate::read_design(&bytes[..length]).is_err());
        }
    }

    #[test]
    fn rejects_duplicate_entries_and_declared_expansion_before_reading_payloads() {
        let bytes = write_design_container(&document()).unwrap();
        let mut entries = parts(&bytes);
        entries.insert("cache/one.bin".into(), vec![1]);
        entries.insert("cache/two.bin".into(), vec![2]);
        let mut duplicate = package(&entries);
        let offsets: Vec<_> = duplicate.windows(13).enumerate().filter_map(|(i, v)| (v == b"cache/two.bin").then_some(i)).collect();
        // Both the local header and central-directory name are changed.
        for i in offsets {
            duplicate[i..i + 13].copy_from_slice(b"cache/one.bin");
        }
        assert!(crate::read_design(&duplicate).is_err());
        let mut bomb = bytes.clone();
        let central = bomb.windows(4).position(|v| v == b"PK\x01\x02").unwrap();
        bomb[central + 24..central + 28].copy_from_slice(&((max_step_part() + 1) as u32).to_le_bytes());
        assert!(crate::read_design(&bomb).unwrap_err().to_string().contains("expanded package too large"));
        let mut excessive = bytes;
        let end = excessive.windows(4).rposition(|v| v == b"PK\x05\x06").unwrap();
        excessive[end + 8..end + 12].copy_from_slice(&[0xff; 4]);
        assert!(crate::read_design(&excessive).is_err());
    }

    /// Printable text that compresses poorly (93 characters: about 6.5 bits each). No `*`, so
    /// it never ends a STEP comment.
    fn noise(length: usize) -> String {
        let mut state = 0x2545_f491_4f6c_dd1du64;
        (0..length)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                let c = b'!' + ((state >> 32) % 93) as u8;
                char::from(if c >= b'*' { c + 1 } else { c })
            })
            .collect()
    }

    /// A valid STEP cube, padded with a comment to `length` bytes. `noisy` padding barely
    /// compresses; otherwise it is repetitive like the text of a real large export.
    fn padded_step(length: usize, noisy: bool) -> String {
        use solidcraft_geom::Vec3;
        let cube = solidcraft_kernel::box_solid(Vec3::ZERO, Vec3::new(10.0, 20.0, 30.0)).unwrap();
        let step = solidcraft_kernel::step_export(&[&cube], "padded").unwrap();
        let (head, tail) = step.split_at(step.find("DATA;").unwrap() + 5);
        let end = length - tail.len() - 2;
        let mut out = String::with_capacity(length);
        out.push_str(head);
        out.push_str("\n/*");
        if noisy {
            out.push_str(&noise(end - out.len()));
        } else {
            let mut line = 0u64;
            while out.len() + 64 < end {
                line += 1;
                out.push_str(&format!("#{line}=CARTESIAN_POINT('',({}.,{}.5,-{}.25));\n", line % 997, line % 89, line % 13));
            }
            out.extend(std::iter::repeat_n(' ', end - out.len()));
        }
        out.push_str("*/");
        out.push_str(tail);
        assert_eq!(out.len(), length);
        out
    }

    /// #3221: a STEP file larger than a design part used to import, then fail to save.
    #[test]
    fn step_over_the_part_limit_imports_saves_compressed_and_reopens() {
        let step = padded_step(crate::MAX_DESIGN_BYTES + (1 << 20), false);
        let imported = crate::step_import_feature(step.as_bytes(), "large.step").unwrap();
        drop(step);
        let mut doc = Document::new("Large import");
        doc.add_feature(imported.kind, None).unwrap();
        let bytes = write_design_container(&doc).unwrap();
        assert!(bytes.len() < crate::MAX_DESIGN_BYTES / 2, "saved {} MB", bytes.len() >> 20);
        let restored = crate::read_design(&bytes).unwrap();
        let (Some(FeatureKind::Import { step: before, .. }), Some(FeatureKind::Import { step: after, .. })) =
            (doc.features.first().map(|f| &f.kind), restored.features.first().map(|f| &f.kind))
        else {
            panic!("the reopened design lost its import");
        };
        assert!(before.len() > crate::MAX_DESIGN_BYTES && before == after, "the reopened STEP text differs");
        let reopened = crate::step_import_feature(after.as_bytes(), "large.step").unwrap();
        assert_eq!(reopened.body_names, imported.body_names);
        assert_eq!(reopened.import.bodies.len(), 1);
    }

    /// #6600: a mesh whose binary form is larger than a design file used to import, then
    /// fail to save (`mesh too large`) before compression was considered.
    #[test]
    fn mesh_larger_than_the_file_saves_compressed_and_reopens() {
        let positions: Arc<[f64]> = Arc::from([0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]);
        let overhead = 24 + positions.len() * 8;
        let min_indices = (crate::MAX_DESIGN_BYTES - overhead) / 4 + 1;
        let indices = min_indices.div_ceil(3) * 3;
        let encoded = overhead + indices * 4;
        assert!(encoded > crate::MAX_DESIGN_BYTES && encoded <= max_mesh_part(), "encoded {encoded}");
        let mesh = MeshData {
            name: "Large".into(),
            positions: Arc::clone(&positions),
            triangles: (0..indices).map(|i| (i % 3) as u32).collect(),
            color: None,
        };
        let mut doc = Document::new("Large mesh");
        doc.add_feature(FeatureKind::MeshImport { file: "large.stl".into(), meshes: vec![mesh] }, None).unwrap();
        for compression in [DesignCompression::Fast, DesignCompression::Compact] {
            let bytes = write_design_container_with_compression(&doc, compression).unwrap();
            assert!(bytes.len() < crate::MAX_DESIGN_BYTES / 2, "saved {} MB", bytes.len() >> 20);
            {
                let mut archive = zip::ZipArchive::new(Cursor::new(bytes.as_slice())).unwrap();
                let index = (0..archive.len())
                    .find(|&i| archive.name_for_index(i).and_then(|name| name.ok()).is_some_and(|name| name.ends_with(".mesh")))
                    .unwrap();
                let file = archive.by_index(index).unwrap();
                assert_eq!(file.compression(), zip::CompressionMethod::Deflated);
                assert_eq!(file.size(), encoded as u64);
            }
            let restored = crate::read_design(&bytes).unwrap();
            assert_eq!(restored, doc, "{compression:?} must preserve every coordinate and triangle index");
        }
    }

    #[test]
    fn mesh_over_the_expanded_cap_is_too_large_before_it_is_encoded() {
        assert_eq!(mesh_encoded_len(0, 0), Some(24));
        let at_cap = (max_mesh_part() - 24) / 4;
        assert_eq!(mesh_encoded_len(0, at_cap), Some(max_mesh_part()));
        assert!(mesh_encoded_len(0, at_cap + 1).is_none());
        let indices = (max_mesh_part() / 4 + 3).div_ceil(3) * 3;
        assert!(mesh_encoded_len(0, indices).is_none());
        assert!(mesh_encoded_len(usize::MAX, 0).is_none());
        assert!(mesh_encoded_len(0, usize::MAX).is_none());
        assert!(mesh_encoded_len(usize::MAX, usize::MAX).is_none());
    }

    #[test]
    fn mesh_archives_keep_expansion_and_repeated_reference_limits() {
        // Change only the declared sizes in a tiny archive: admission must reject these
        // without allocating or decompressing a half-gigabyte payload.
        let entries = BTreeMap::from([("blobs/a.mesh".into(), vec![0]), ("blobs/b.mesh".into(), vec![0])]);
        let original = package(&entries);
        let headers: Vec<_> = original.windows(4).enumerate().filter_map(|(i, v)| (v == b"PK\x01\x02").then_some(i)).collect();
        assert_eq!(headers.len(), 2);
        for sizes in [[max_mesh_part() + 1, 1], [max_mesh_part(), max_mesh_part()]] {
            let mut bytes = original.clone();
            for (header, size) in headers.iter().zip(sizes) {
                bytes[header + 24..header + 28].copy_from_slice(&(size as u32).to_le_bytes());
            }
            let error = PackageReader::new(Cursor::new(bytes)).err().unwrap();
            assert!(error.to_string().contains("expanded package too large"), "{error}");
        }
        let mut doc = Document::new("Mesh budget");
        doc.add_feature(FeatureKind::MeshImport { file: "triangle.stl".into(), meshes: vec![mesh()] }, None).unwrap();
        let bytes = write_design_container(&doc).unwrap();
        let entries = parts(&bytes);
        let metadata: Value = serde_json::from_slice(&entries["document.json"]).unwrap();
        let reference = &metadata["features"][0]["meshes"][0];
        let mut reader = PackageReader::new(Cursor::new(&bytes)).unwrap();
        let length = reader.blob(reference, "mesh_blob", "mesh").unwrap().len();
        reader.hydrated = max_expanded() - length;
        assert!(reader.blob(reference, "mesh_blob", "mesh").is_ok());
        assert!(reader.blob(reference, "mesh_blob", "mesh").unwrap_err().to_string().contains("resolved payloads too large"));
    }

    #[test]
    fn smaller_meshes_keep_the_compression_policy_and_older_designs_reopen() {
        let mut doc = Document::new("Small mesh");
        doc.add_feature(FeatureKind::MeshImport { file: "triangle.stl".into(), meshes: vec![mesh()] }, None).unwrap();
        for (policy, method) in
            [(DesignCompression::Fast, zip::CompressionMethod::Stored), (DesignCompression::Compact, zip::CompressionMethod::Deflated)]
        {
            let bytes = write_design_container_with_compression(&doc, policy).unwrap();
            let mut entries = parts(&bytes);
            let mut metadata: Value = serde_json::from_slice(&entries["document.json"]).unwrap();
            assert_eq!(metadata["format"], solidcraft_doc::format::format_string(solidcraft_doc::format::FORMAT));
            let blob = metadata["features"][0]["meshes"][0]["mesh_blob"].as_str().unwrap();
            let mut archive = zip::ZipArchive::new(Cursor::new(&bytes)).unwrap();
            assert_eq!(archive.by_name(blob).unwrap().compression(), method);
            // Format 36 is the last release before large mesh blobs. Its payload needs no
            // migration, in either stored or compressed form.
            metadata["format"] = json!("solidcraft/36");
            metadata.as_object_mut().unwrap().remove("format_pending");
            entries.insert("document.json".into(), serde_json::to_vec(&metadata).unwrap());
            let mut legacy = zip::ZipWriter::new(Cursor::new(Vec::new()));
            for (name, data) in entries {
                legacy.start_file(name, zip::write::SimpleFileOptions::default().compression_method(method)).unwrap();
                legacy.write_all(&data).unwrap();
            }
            assert_eq!(crate::read_design(&legacy.finish().unwrap().into_inner()).unwrap(), doc);
        }
    }

    #[test]
    fn step_that_cannot_be_saved_even_compressed_is_refused_at_import() {
        let step = padded_step(crate::MAX_DESIGN_BYTES + (80 << 20), true);
        let message = crate::step_import_feature(step.as_bytes(), "noisy.step").err().unwrap().to_string();
        assert!(message.contains("too large to save in a design"), "{message}");
    }

    #[test]
    fn designs_with_uncompressed_step_sources_still_open() {
        // Saves before #3221 stored the STEP source without compression.
        let doc = document();
        let entries = parts(&write_design_container(&doc).unwrap());
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, bytes) in &entries {
            let method = if name.ends_with(".step") { zip::CompressionMethod::Stored } else { zip::CompressionMethod::Deflated };
            zip.start_file(name, zip::write::SimpleFileOptions::default().compression_method(method)).unwrap();
            zip.write_all(bytes).unwrap();
        }
        let old = zip.finish().unwrap().into_inner();
        assert!(old.windows(13).any(|v| v == b"ISO-10303-21;"));
        assert_eq!(crate::read_design(&old).unwrap(), doc);
    }

    #[test]
    fn decompression_bomb_is_refused_without_expanding_it() {
        // 64 MiB that deflate to a few hundred kilobytes, declared as a 1 KiB STEP source.
        let mut doc = Document::new("Bomb");
        let step = format!("ISO-10303-21;{}", " ".repeat(64 << 20));
        doc.add_feature(FeatureKind::Import { file: "bomb.step".into(), step: step.into(), components: Vec::new(), skip: Vec::new() }, None).unwrap();
        let honest = write_design_container(&doc).unwrap();
        assert!(honest.len() < 1 << 20);
        let central = honest.windows(4).position(|v| v == b"PK\x01\x02").unwrap();
        let mut bomb = honest.clone();
        // The STEP source is the first part: its local header is at 0.
        bomb[22..26].copy_from_slice(&1024u32.to_le_bytes());
        bomb[central + 24..central + 28].copy_from_slice(&1024u32.to_le_bytes());
        let message = crate::read_design(&bomb).unwrap_err().to_string();
        assert!(message.contains("declared uncompressed size") || message.contains("length mismatch"), "{message}");
        // An honest declaration over the limit is refused before the part is read at all.
        let mut over = honest;
        over[central + 24..central + 28].copy_from_slice(&((max_step_part() + 1) as u32).to_le_bytes());
        assert!(crate::read_design(&over).unwrap_err().to_string().contains("expanded package too large"));
    }

    #[test]
    fn rejects_invalid_references_and_repeated_payload_expansion() {
        let bytes = write_design_container(&document()).unwrap();
        let mut entries = parts(&bytes);
        let mut metadata: Value = serde_json::from_slice(&entries["document.json"]).unwrap();
        let reference = metadata["features"][0].clone();
        let mut reader = PackageReader::new(Cursor::new(&bytes)).unwrap();
        reader.hydrated = max_expanded();
        assert!(reader.blob(&reference, "step_blob", "step").unwrap_err().to_string().contains("resolved payloads too large"));
        for name in ["../escape.step", "blobs/not-a-hash.step", "blobs/00.png"] {
            metadata["features"][0]["step_blob"] = json!(name);
            entries.insert("document.json".into(), serde_json::to_vec(&metadata).unwrap());
            assert!(crate::read_design(&package(&entries)).is_err());
        }
    }

    #[test]
    fn deterministic_archive_mutations_never_panic() {
        let preview = DesignPreview { meshes: vec![mesh()] };
        let bytes = write_design_container_with_preview(&document(), DesignCompression::Fast, Some(&preview)).unwrap();
        assert!(read_design_preview(&bytes).is_some());
        for end in 0..bytes.len() {
            assert!(
                std::panic::catch_unwind(|| {
                    let _ = crate::read_design(&bytes[..end]);
                    let _ = read_design_preview(&bytes[..end]);
                    if let Ok(mut reader) = DesignContainerReader::new(Cursor::new(&bytes[..end])) {
                        let _ = reader.preview();
                        let _ = reader.read_document();
                    }
                })
                .is_ok(),
                "truncated at {end}"
            );
        }
        let mut seed = 0x303_u64;
        for _ in 0..512 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let mut changed = bytes.clone();
            let index = seed as usize % changed.len();
            changed[index] ^= (seed >> 32) as u8 | 1;
            assert!(
                std::panic::catch_unwind(|| {
                    let _ = crate::read_design(&changed);
                    let _ = read_design_preview(&changed);
                    if let Ok(mut reader) = DesignContainerReader::new(Cursor::new(&changed)) {
                        let _ = reader.preview();
                        let _ = reader.read_document();
                    }
                })
                .is_ok()
            );
        }
    }

    #[test]
    fn metadata_round_trip_preserves_new_and_existing_fields() {
        let mut doc = Document::new("Empty");
        doc.units = "in".into();
        doc.named_views.push(solidcraft_doc::NamedView { name: "View".into(), ..Default::default() });
        doc.param_comments.insert("test".into(), "metadata".into());
        let setup = solidcraft_doc::render_settings::RenderSetup { exposure_ev: 1.5, ..Default::default() };
        std::sync::Arc::make_mut(&mut doc.render).setups.insert("Studio".into(), setup);
        // Design properties and other tools' data (#1015), kept as they are in both forms.
        doc.metadata = serde_json::from_value(json!({
            "author": 42,
            "revision": "B",
            "com.acme.description": {"shop_id": 88123, "list": [1, 2.5, null, true, "x", {"deep": []}]},
        }))
        .unwrap();
        let expected = crate::read_design(&crate::write_design(&doc)).unwrap();
        assert_eq!(expected, doc);
        assert_eq!(crate::read_design(&write_design_container(&doc).unwrap()).unwrap(), expected);
    }

    #[test]
    fn portable_numerical_render_jobs_survive_json_and_design_container_reopen() {
        let mut doc = document();
        let before = doc.clone();
        doc.render = std::sync::Arc::new(serde_json::from_value(json!({"setups":{
            "CPU":{"exposure_ev":1.25,"job":{"method":"cpu_raytrace","options":{"primary_samples":4,"lights":[],"shadows":false,
                "area_lights":[{"center_mm":[0,0,20],"edge_u_mm":[10,0,0],"edge_v_mm":[0,-10,0],"color":[0.4,0.6,1],"intensity":2}],"area_samples":16,"seed":123}}},
            "GPU":{"exposure_ev":-1.5,"job":{"method":"gpu_path","transparent_background":true,"options":{"samples":91,"max_depth":11,"seed":456,
                "tone_map":"aces_fitted","denoise":true,"lighting":{"environment":[0,0,0],"area_lights":[]},
                "environment":{"kind":"sky","rotation_rad":0.25,"intensity":0.5,"gradient":{"zenith":[0.4,0.2,0.1],"horizon":[0.7,0.6,0.3],"ground":[0.2,0.1,0.05]}}}}}
        }})).unwrap());
        assert!(doc.differs_only_in_render(&before));
        for bytes in [crate::write_design(&doc), write_design_container(&doc).unwrap()] {
            let reopened = crate::read_design(&bytes).unwrap();
            assert_eq!(reopened, doc);
            assert_eq!(reopened.render, doc.render);
        }
    }

    #[test]
    fn embedded_hdr_is_one_required_blob_shared_by_cpu_gpu_and_strictly_hydrated() {
        use solidcraft_doc::{render_resources::HdrResource, render_settings::RenderSettings};
        let mut source = b"#?RADIANCE\nFORMAT=32-bit_rle_rgbe\nPRIMARIES=0.64 0.33 0.30 0.60 0.15 0.06 0.3127 0.3290\n\n-Y 2 +X 4\n".to_vec();
        for _ in 0..8 {
            source.extend_from_slice(&[128, 64, 32, 129]);
        }
        let resource = HdrResource::new(source.clone()).unwrap();
        let id = resource.id().to_string();
        let mut render = RenderSettings::default();
        render.resources.insert(resource).unwrap();
        for (name, job) in [
            ("CPU", json!({"method":"cpu_raytrace","options":{"lights":[],"hdr":{"resource":id,"rotation_rad":0.75,"intensity":2,"samples":64}}})),
            (
                "GPU",
                json!({"method":"gpu_path","options":{"samples":37,"hdr":{"resource":id,"rotation_rad":-0.25,"intensity":1,"assume_linear_srgb":false}}}),
            ),
        ] {
            render.setups.insert(name.into(), serde_json::from_value(json!({"exposure_ev":1.25,"job":job})).unwrap());
        }
        let mut doc = document();
        doc.render = Arc::new(render);
        let bytes = write_design_container(&doc).unwrap();
        assert_eq!(crate::read_design(&bytes).unwrap(), doc);
        assert_eq!(crate::read_design(&crate::write_design(&doc)).unwrap(), doc);
        let original = parts(&bytes);
        let blob = format!("blobs/{id}.hdr");
        assert_eq!(original.keys().filter(|name| name.ends_with(".hdr")).count(), 1);
        assert_eq!(original[&blob], source);
        let metadata: Value = serde_json::from_slice(&original["document.json"]).unwrap();
        assert_eq!(metadata["render"]["resources"][&id], json!({"kind":"radiance_hdr","data":"","hdr_blob":blob}));
        assert!(Document::from_json(&metadata.to_string()).is_err(), "container metadata is not standalone JSON");
        let mut missing = original.clone();
        missing.remove(&blob);
        assert!(crate::read_design(&package(&missing)).unwrap_err().to_string().contains("required part"));
        let mut corrupt = original.clone();
        corrupt.get_mut(&blob).unwrap()[0] ^= 1;
        assert!(crate::read_design(&package(&corrupt)).unwrap_err().to_string().contains("hash mismatch"));
        for (field, value) in
            [("kind", json!("png")), ("data", json!("AAAA")), ("path", json!("original.hdr")), ("hdr_blob", json!(format!("blobs/{id}.png")))]
        {
            let mut entries = original.clone();
            let mut altered = metadata.clone();
            altered["render"]["resources"][&id][field] = value;
            entries.insert("document.json".into(), serde_json::to_vec(&altered).unwrap());
            assert!(crate::read_design(&package(&entries)).is_err(), "{field}");
        }
        let mut entries = original.clone();
        let mut altered = metadata.clone();
        let value = altered["render"]["resources"].as_object_mut().unwrap().remove(&id).unwrap();
        altered["render"]["resources"]["0".repeat(64)] = value;
        entries.insert("document.json".into(), serde_json::to_vec(&altered).unwrap());
        assert!(crate::read_design(&package(&entries)).unwrap_err().to_string().contains("resource key"));
        let entry = &metadata["render"]["resources"][&id];
        let duplicate = format!("{{\"render\":{{\"resources\":{{\"{id}\":{entry},\"{id}\":{entry}}}}}}}");
        entries.insert("document.json".into(), duplicate.into_bytes());
        assert!(crate::read_design(&package(&entries)).unwrap_err().to_string().contains("duplicate"));
    }

    #[test]
    fn embedded_hdr_container_refuses_oversized_part_before_reading_payload() {
        use solidcraft_doc::render_resources::{HdrId, MAX_HDR_BYTES};
        let source = vec![0u8; MAX_HDR_BYTES + 1];
        let id = HdrId::of(&source).to_string();
        let blob = format!("blobs/{id}.hdr");
        let mut metadata = serde_json::to_value(Document::default()).unwrap();
        metadata["render"] = json!({"setups":{},"resources":{(id):{"kind":"radiance_hdr","data":"","hdr_blob":blob}}});
        let entries = BTreeMap::from([
            ("container.json".into(), serde_json::to_vec(&json!({"format":VERSION})).unwrap()),
            ("document.json".into(), serde_json::to_vec(&metadata).unwrap()),
            (blob, source),
        ]);
        let bytes = package(&entries);
        let count = std::rc::Rc::new(std::cell::Cell::new(0));
        let reader = DesignContainerReader::new(CountingReader { source: Cursor::new(bytes), count: count.clone() }).unwrap();
        assert!(reader.read_document().unwrap_err().to_string().contains("part too large"));
        assert!(count.get() < 256 * 1024, "read {} bytes before refusing an oversized HDR", count.get());
    }

    #[test]
    fn embedded_hdr_manifest_rejects_escaped_inline_payload_before_value_materialization() {
        let id = "0".repeat(64);
        let escaped = "\\u0041".repeat(4096);
        let wire = format!(
            "{{\"render\":{{\"resources\":{{\"{id}\":{{\"kind\":\"radiance_hdr\",\"data\":\"{escaped}\",\"hdr_blob\":\"blobs/{id}.hdr\"}}}}}}}}"
        );
        assert!(check_hdr_manifest(wire.as_bytes()).unwrap_err().to_string().contains("marker exceeds its budget"));
        let oversized = "\\u0030".repeat(4096);
        let wire = format!("{{\"render\":{{\"resources\":{{\"{id}\":{{\"kind\":\"radiance_hdr\",\"data\":\"\",\"hdr_blob\":\"{oversized}\"}}}}}}}}");
        assert!(check_hdr_manifest(wire.as_bytes()).unwrap_err().to_string().contains("marker exceeds its budget"));
    }

    /// Public-safe generated data, no geometry evaluation. Print medians without
    /// timing assertions, which would be unreliable on shared build machines.
    #[test]
    #[ignore = "manual IO performance measurement"]
    fn benchmark_design_storage() {
        let mut doc = Document::new("Generated mesh benchmark");
        let mut large = mesh();
        large.positions = (0..300_000).map(|i| (i as f64 * 0.001234567890123).sin() * 100.0).collect();
        large.triangles = (0..300_000).map(|i| (i % 100_000) as u32).collect();
        doc.add_feature(FeatureKind::MeshImport { file: "generated.obj".into(), meshes: vec![large] }, None).unwrap();
        measure_storage(&doc, "mesh");
    }

    #[test]
    #[ignore = "manual IO performance measurement"]
    fn benchmark_step_storage() {
        use solidcraft_geom::Vec3;
        let bodies: Vec<_> = (0..512)
            .map(|i| {
                let corner = Vec3::new((i % 32) as f64 * 12.0, (i / 32) as f64 * 12.0, 0.0);
                solidcraft_kernel::box_solid(corner, corner + Vec3::new(10.0, 10.0, 10.0)).unwrap()
            })
            .collect();
        let step = solidcraft_kernel::step_export(&bodies.iter().collect::<Vec<_>>(), "SolidCraft storage benchmark").unwrap();
        let mut doc = Document::new("Generated STEP benchmark");
        doc.add_feature(FeatureKind::Import { file: "generated.step".into(), step: step.into(), components: Vec::new(), skip: Vec::new() }, None)
            .unwrap();
        measure_storage(&doc, "step");
    }

    fn measure_storage(doc: &Document, label: &str) {
        let json = crate::write_design(doc);
        let container = write_design_container(doc).unwrap();
        let dir = std::env::var_os("CARGO_TARGET_DIR").map(std::path::PathBuf::from).unwrap_or_else(|| std::path::PathBuf::from("target"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("issue303-{label}.json")), &json).unwrap();
        std::fs::write(dir.join(format!("issue303-{label}.solidcraft")), &container).unwrap();
        let mut timings = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];
        for _ in 0..9 {
            let start = std::time::Instant::now();
            std::hint::black_box(crate::write_design(doc));
            timings[0].push(start.elapsed().as_secs_f64() * 1000.0);
            let start = std::time::Instant::now();
            std::hint::black_box(write_design_container(doc).unwrap());
            timings[1].push(start.elapsed().as_secs_f64() * 1000.0);
            let start = std::time::Instant::now();
            let plain = crate::read_design(&json).unwrap();
            timings[2].push(start.elapsed().as_secs_f64() * 1000.0);
            let start = std::time::Instant::now();
            let zipped = crate::read_design(&container).unwrap();
            timings[3].push(start.elapsed().as_secs_f64() * 1000.0);
            assert_eq!(plain, zipped);
        }
        for times in &mut timings {
            times.sort_by(f64::total_cmp);
        }
        eprintln!(
            "{label}: bytes json={} container={}; median ms save_json={:.3} save_container={:.3} read_json={:.3} read_container={:.3}",
            json.len(),
            container.len(),
            timings[0][4],
            timings[1][4],
            timings[2][4],
            timings[3][4]
        );
        let mut saves = [Vec::new(), Vec::new()];
        for round in 0..9 {
            // Alternate ordering; include encoding, flushing, replacement and backup.
            for index in if round % 2 == 0 { [0, 1] } else { [1, 0] } {
                let start = std::time::Instant::now();
                let bytes = if index == 0 { crate::write_design(doc) } else { write_design_container(doc).unwrap() };
                crate::write_atomic(&dir.join(format!("issue303-{label}-atomic-{index}.solidcraft")), &bytes, true).unwrap();
                saves[index].push(start.elapsed().as_secs_f64() * 1000.0);
            }
        }
        for times in &mut saves {
            times.sort_by(f64::total_cmp);
        }
        eprintln!("{label}: median atomic-save ms json={:.3} container={:.3}", saves[0][4], saves[1][4]);
    }
}
