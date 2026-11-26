//! Drawing files (#3890): a [`Drawing`] saved as a container or as plain JSON, like a design.
//!
//! A drawing container is a design container holding `drawing.json` (Deflate, as
//! `document.json`) in place of `document.json`, and saying so in `container.json`:
//! `{"content": "drawing", "format": "solidcraft-container/1"}`. A design's `container.json` has no
//! `content`, as before. The container's own checks (budgets, entries, the ZIP directory) are the
//! design container's. The plain JSON form is [`Drawing::to_json`]'s text, the same bytes each
//! save, so a drawing can live in version control as text.
//!
//! A design is never read as a drawing nor a drawing as a design: [`file_kind`] tells them apart,
//! and each reader refuses the other's file by name.
//!
//! A design's drawings sit in its container (#3890): each drawing in its own entry,
//! `drawings/<id>.json` (Deflate, as `document.json`), its id as [`ids::encode`] writes it and its
//! text [`Drawing::to_json`]'s. `document.json` holds nothing of them, so its bytes, and the model
//! cache keyed to them, don't change when only a drawing does. An entry under `drawings/` that
//! isn't a drawing's, or whose record has another id, is damage: the design is refused.

use std::io::{Cursor, Read, Seek};
use std::sync::Arc;

use serde::Deserialize;
use serde_json::{Value, json};
use solidcraft_doc::drawing::{self, Drawing, MAX_DRAWING_BYTES};
use solidcraft_doc::format::DRAWING;
use solidcraft_doc::{Document, ids};

use super::{PackageReader, PackageWriter, VERSION, invalid, is_design_container};
use crate::{IoError, MAX_DESIGN_BYTES, Result};

/// The part a drawing container keeps its drawing in.
const DRAWING_PART: &str = "drawing.json";
/// What a drawing's `container.json` says it holds.
const DRAWING_CONTENT: &str = "drawing";
/// The folder of a design container's drawing entries.
const FOLDER: &str = "drawings/";

/// What a SolidCraft file holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileKind {
    Design,
    Drawing,
}

/// Which kind of file `bytes` are, from the container's `container.json` or the JSON's `format`.
/// JSON that isn't a drawing's counts as a design, which the design reader then checks.
pub fn file_kind(bytes: &[u8]) -> Result<FileKind> {
    if is_design_container(bytes) {
        return container_kind(&mut PackageReader::new(Cursor::new(bytes))?);
    }
    #[derive(Deserialize)]
    struct Format {
        format: Option<String>,
    }
    let format = serde_json::from_slice::<Format>(bytes).ok().and_then(|f| f.format);
    Ok(if format.is_some_and(|f| DRAWING.version_of(&f).is_some()) { FileKind::Drawing } else { FileKind::Design })
}

/// What a container holds, by its `container.json`: no `content` is a design.
pub(super) fn container_kind<R: Read + Seek>(reader: &mut PackageReader<R>) -> Result<FileKind> {
    let container: Value = serde_json::from_slice(&reader.limited_part("container.json", 4096)?).map_err(invalid)?;
    if !container.get("format").and_then(Value::as_str).is_some_and(super::is_container_version) {
        return Err(invalid("unsupported container version"));
    }
    match container.get("content") {
        None => Ok(FileKind::Design),
        Some(Value::String(content)) if content == DRAWING_CONTENT => Ok(FileKind::Drawing),
        // A later kind of content that still holds a design opens as one.
        Some(_) if reader.has_part("document.json") => Ok(FileKind::Design),
        Some(other) => Err(invalid(format!("it holds {other}, which this version doesn't know: update SolidCraft to open it"))),
    }
}

/// The drawing as a container: the same bytes for the same drawing.
pub fn write_drawing_container(drawing: &Drawing) -> Result<Vec<u8>> {
    let mut writer = PackageWriter::new();
    let text = record(&mut writer, drawing)?;
    writer.part(DRAWING_PART, text.as_bytes(), true)?;
    let container = json!({"format": VERSION, "content": DRAWING_CONTENT});
    writer.part("container.json", &serde_json::to_vec(&container).map_err(invalid)?, false)?;
    writer.finish()
}

/// Read a drawing file's bytes: a container or plain JSON.
pub fn read_drawing(bytes: &[u8]) -> Result<Drawing> {
    if bytes.len() > MAX_DESIGN_BYTES {
        return Err(IoError::Invalid("drawing file too large".into()));
    }
    if is_design_container(bytes) {
        return read_container(Cursor::new(bytes));
    }
    if file_kind(bytes)? == FileKind::Design {
        return Err(IoError::Invalid("this file is a SolidCraft design, not a drawing: open it as a design".into()));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| IoError::Invalid("drawing file is not UTF-8 JSON".into()))?;
    Ok(Drawing::from_json(text)?)
}

/// Read a drawing from disk (or the browser's files), a container through one seekable handle.
pub fn read_drawing_file(path: &str) -> Result<Drawing> {
    let mut source = super::open_regular_file(path)?;
    if super::is_container_source(&mut source)? {
        return read_container(source);
    }
    let mut bytes = Vec::new();
    source.take(MAX_DRAWING_BYTES as u64 + 1).read_to_end(&mut bytes).map_err(invalid)?;
    read_drawing(&bytes)
}

fn read_container<R: Read + Seek>(source: R) -> Result<Drawing> {
    let mut reader = PackageReader::new(source)?;
    if container_kind(&mut reader)? == FileKind::Design {
        return Err(IoError::Invalid("this file is a SolidCraft design, not a drawing: open it as a design".into()));
    }
    if reader.has_part("document.json") {
        return Err(invalid("a drawing container also holds a design's document.json"));
    }
    let bytes = reader.limited_part(DRAWING_PART, MAX_DRAWING_BYTES)?;
    open_record(&mut reader, &bytes)
}

/// A drawing's record for a container, its image files written as the container's parts it names:
/// never as text.
fn record(writer: &mut PackageWriter, d: &Drawing) -> Result<String> {
    let (text, parts) = solidcraft_doc::blob::collecting_parts(|| d.to_json());
    let text = text.map_err(|e| invalid(format!("{} not saved: {e}", d.label())))?;
    for (name, bytes) in parts {
        let extension = name.rsplit_once('.').map_or("", |(_, extension)| extension);
        let written = writer.blob(&bytes, extension, false)?;
        if written.as_str() != Some(name.as_str()) {
            return Err(invalid(format!("{} not saved: its image part is named {written}, not {name}", d.label())));
        }
    }
    Ok(text)
}

/// Reads a drawing's record from a container, its image files from the parts it names, as bytes.
fn open_record<R: Read + Seek>(reader: &mut PackageReader<R>, bytes: &[u8]) -> Result<Drawing> {
    if bytes.len() > MAX_DRAWING_BYTES {
        return Err(invalid(format!("drawing too large (at most {} MiB)", MAX_DRAWING_BYTES >> 20)));
    }
    let meta = extract_drawing_meta(bytes).map_err(|e| invalid(format!("not a SolidCraft drawing: {e}")))?;
    let mut parts = std::collections::BTreeMap::new();
    // Set aside where a release that can't read them looks past them (`image_files`), or, as nightly
    // builds saved them for a while, in `images`.
    let images = meta.image_files.as_deref().or(meta.images.as_deref()).unwrap_or(&[]);
    if images.len() > drawing::image::MAX_IMAGES {
        return Err(invalid(format!("a drawing has at most {} images", drawing::image::MAX_IMAGES)));
    }
    // The drawing's own budget, kept while the parts are read, before any is held.
    let mut total = 0usize;
    for image in images {
        let (Some(name), Some(format)) = (image.part.as_deref(), image.format.as_deref()) else { continue };
        if !matches!(format, "png" | "jpeg") || parts.contains_key(name) {
            continue;
        }
        let room = drawing::image::MAX_IMAGES_BYTES.saturating_sub(total).min(drawing::image::MAX_IMAGE_BYTES);
        let blob = reader.limited_blob_named(name, format, room)?;
        total += blob.len();
        parts.insert(name.to_string(), solidcraft_doc::blob::Blob::from(blob));
    }
    // A newer record can name other blob types, even in a drawing this build can open.
    // Resolve only parts present in this container and use its usual size and digest checks.
    if newer_meta(&meta).is_some() {
        for name in blob_names_in_json(bytes)? {
            if !parts.contains_key(&name)
                && reader.has_part(&name)
                && let Some((_, extension)) = name.rsplit_once('.')
            {
                let blob = reader.limited_blob_named(&name, extension, super::part_limit(&name))?;
                parts.insert(name, solidcraft_doc::blob::Blob::from(blob));
            }
        }
    }
    let text = std::str::from_utf8(bytes).map_err(invalid)?;
    Ok(solidcraft_doc::blob::with_parts(parts, || Drawing::from_json(text))?)
}

/// Write each of the design's drawings to its entry.
pub(super) fn write(writer: &mut PackageWriter, doc: &Document) -> Result<()> {
    drawing::check_in_design(&doc.drawings)?;
    for d in &doc.drawings {
        let text = record(writer, d)?;
        writer.part(&format!("{FOLDER}{}.json", ids::encode(d.id)), text.as_bytes(), true)?;
    }
    Ok(())
}

/// A drawing a newer build saved that this build can't read at all: its entry and the parts it
/// names, kept as they came, and what to say about it.
pub(super) struct KeptDrawing {
    pub parts: Vec<(String, Arc<[u8]>)>,
    pub note: String,
}

/// Read the container's drawings, in their order. A drawing a newer build saved that this build
/// can't read is kept as it came, with the parts it names (`KeptDrawing`), so the rest of the
/// design opens and saving writes it back; one this build's own format says is damaged refuses
/// the design, as damage does.
pub(super) fn read<R: Read + Seek>(reader: &mut PackageReader<R>) -> Result<(Vec<Arc<Drawing>>, Vec<KeptDrawing>)> {
    let names: Vec<String> = reader.part_names().into_iter().filter(|n| n.starts_with(FOLDER)).collect();
    drawing::check_count(names.len()).map_err(invalid)?;
    let mut drawings = Vec::with_capacity(names.len());
    let mut kept = Vec::new();
    for name in names {
        let id = name.strip_prefix(FOLDER).and_then(|n| n.strip_suffix(".json")).and_then(ids::decode);
        let id = id.ok_or_else(|| invalid(format!("`{name}` is not a drawing's entry")))?;
        let bytes = reader.limited_part(&name, MAX_DRAWING_BYTES)?;
        let d = match open_record(reader, &bytes) {
            Ok(d) => d,
            Err(why) => match extract_drawing_meta(&bytes).ok().as_ref().and_then(newer_meta) {
                Some((label, format)) => {
                    kept.push(keep(reader, &name, &bytes, &label, format, &why)?);
                    continue;
                }
                None => return Err(invalid(format!("`{name}`: {why}"))),
            },
        };
        if d.id != id {
            return Err(invalid(format!("`{name}` holds the drawing {}", ids::encode(d.id))));
        }
        drawings.push(d);
    }
    Ok((drawing::in_design(drawings).map_err(invalid)?, kept))
}

#[derive(Default)]
struct DrawingImageRef {
    part: Option<String>,
    format: Option<String>,
}

#[derive(Default)]
struct DrawingMeta {
    format: Option<String>,
    format_pending: Option<Vec<String>>,
    name: Option<String>,
    image_files: Option<Vec<DrawingImageRef>>,
    images: Option<Vec<DrawingImageRef>>,
}

/// The name and format number of a drawing record a newer build saved: a format this build doesn't
/// read, or changes it doesn't know. `None` for a record this build's own format covers.
fn newer_meta(meta: &DrawingMeta) -> Option<(String, u32)> {
    let version = meta.format.as_deref().and_then(|f| DRAWING.version_of(f))?;
    let newer = DRAWING.newer(version, meta.format_pending.as_deref())?;
    let label = meta.name.as_deref().unwrap_or("unnamed").chars().take(80).collect();
    Some((label, newer.format))
}

/// Stream-extract only header and image-part references from a drawing JSON buffer without
/// allocating an untyped `serde_json::Value` tree.
fn extract_drawing_meta(bytes: &[u8]) -> std::result::Result<DrawingMeta, serde_json::Error> {
    use serde::de::{self, DeserializeSeed, Deserializer, IgnoredAny, MapAccess, SeqAccess, Visitor};

    struct OptStringSeed;
    impl<'de> DeserializeSeed<'de> for OptStringSeed {
        type Value = Option<String>;
        fn deserialize<D: Deserializer<'de>>(self, d: D) -> std::result::Result<Option<String>, D::Error> {
            struct V;
            impl<'de> Visitor<'de> for V {
                type Value = Option<String>;
                fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    f.write_str("optional string")
                }
                fn visit_str<E: de::Error>(self, v: &str) -> std::result::Result<Option<String>, E> {
                    Ok(Some(v.to_owned()))
                }
                fn visit_string<E: de::Error>(self, v: String) -> std::result::Result<Option<String>, E> {
                    Ok(Some(v))
                }
                fn visit_bool<E: de::Error>(self, _: bool) -> std::result::Result<Option<String>, E> {
                    Ok(None)
                }
                fn visit_i64<E: de::Error>(self, _: i64) -> std::result::Result<Option<String>, E> {
                    Ok(None)
                }
                fn visit_u64<E: de::Error>(self, _: u64) -> std::result::Result<Option<String>, E> {
                    Ok(None)
                }
                fn visit_f64<E: de::Error>(self, _: f64) -> std::result::Result<Option<String>, E> {
                    Ok(None)
                }
                fn visit_unit<E: de::Error>(self) -> std::result::Result<Option<String>, E> {
                    Ok(None)
                }
                fn visit_none<E: de::Error>(self) -> std::result::Result<Option<String>, E> {
                    Ok(None)
                }
                fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<Option<String>, A::Error> {
                    while seq.next_element::<IgnoredAny>()?.is_some() {}
                    Ok(None)
                }
                fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> std::result::Result<Option<String>, A::Error> {
                    while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
                    Ok(None)
                }
            }
            d.deserialize_any(V)
        }
    }

    struct PendingSeed;
    impl<'de> DeserializeSeed<'de> for PendingSeed {
        type Value = Option<Vec<String>>;
        fn deserialize<D: Deserializer<'de>>(self, d: D) -> std::result::Result<Option<Vec<String>>, D::Error> {
            struct V;
            impl<'de> Visitor<'de> for V {
                type Value = Option<Vec<String>>;
                fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    f.write_str("optional pending list")
                }
                fn visit_bool<E: de::Error>(self, _: bool) -> std::result::Result<Self::Value, E> {
                    Ok(None)
                }
                fn visit_i64<E: de::Error>(self, _: i64) -> std::result::Result<Self::Value, E> {
                    Ok(None)
                }
                fn visit_u64<E: de::Error>(self, _: u64) -> std::result::Result<Self::Value, E> {
                    Ok(None)
                }
                fn visit_f64<E: de::Error>(self, _: f64) -> std::result::Result<Self::Value, E> {
                    Ok(None)
                }
                fn visit_str<E: de::Error>(self, _: &str) -> std::result::Result<Self::Value, E> {
                    Ok(None)
                }
                fn visit_unit<E: de::Error>(self) -> std::result::Result<Self::Value, E> {
                    Ok(None)
                }
                fn visit_none<E: de::Error>(self) -> std::result::Result<Self::Value, E> {
                    Ok(None)
                }
                fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> std::result::Result<Self::Value, A::Error> {
                    while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
                    Ok(None)
                }
                fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<Self::Value, A::Error> {
                    let mut list = Vec::new();
                    while let Some(item) = seq.next_element_seed(OptStringSeed)? {
                        if let Some(s) = item
                            && list.len() < 1000
                        {
                            list.push(s);
                        }
                    }
                    Ok(Some(list))
                }
            }
            d.deserialize_any(V)
        }
    }

    struct ImageItemSeed;
    impl<'de> DeserializeSeed<'de> for ImageItemSeed {
        type Value = DrawingImageRef;
        fn deserialize<D: Deserializer<'de>>(self, d: D) -> std::result::Result<DrawingImageRef, D::Error> {
            struct V;
            impl<'de> Visitor<'de> for V {
                type Value = DrawingImageRef;
                fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    f.write_str("image record")
                }
                fn visit_bool<E: de::Error>(self, _: bool) -> std::result::Result<Self::Value, E> {
                    Ok(DrawingImageRef::default())
                }
                fn visit_i64<E: de::Error>(self, _: i64) -> std::result::Result<Self::Value, E> {
                    Ok(DrawingImageRef::default())
                }
                fn visit_u64<E: de::Error>(self, _: u64) -> std::result::Result<Self::Value, E> {
                    Ok(DrawingImageRef::default())
                }
                fn visit_f64<E: de::Error>(self, _: f64) -> std::result::Result<Self::Value, E> {
                    Ok(DrawingImageRef::default())
                }
                fn visit_str<E: de::Error>(self, _: &str) -> std::result::Result<Self::Value, E> {
                    Ok(DrawingImageRef::default())
                }
                fn visit_unit<E: de::Error>(self) -> std::result::Result<Self::Value, E> {
                    Ok(DrawingImageRef::default())
                }
                fn visit_none<E: de::Error>(self) -> std::result::Result<Self::Value, E> {
                    Ok(DrawingImageRef::default())
                }
                fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<Self::Value, A::Error> {
                    while seq.next_element::<IgnoredAny>()?.is_some() {}
                    Ok(DrawingImageRef::default())
                }
                fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> std::result::Result<Self::Value, A::Error> {
                    let mut item = DrawingImageRef::default();
                    while let Some(key) = map.next_key::<String>()? {
                        match key.as_str() {
                            "part" => item.part = map.next_value_seed(OptStringSeed)?,
                            "format" => item.format = map.next_value_seed(OptStringSeed)?,
                            _ => {
                                map.next_value::<IgnoredAny>()?;
                            }
                        }
                    }
                    Ok(item)
                }
            }
            d.deserialize_any(V)
        }
    }

    struct ImagesSeed;
    impl<'de> DeserializeSeed<'de> for ImagesSeed {
        type Value = Option<Vec<DrawingImageRef>>;
        fn deserialize<D: Deserializer<'de>>(self, d: D) -> std::result::Result<Option<Vec<DrawingImageRef>>, D::Error> {
            struct V;
            impl<'de> Visitor<'de> for V {
                type Value = Option<Vec<DrawingImageRef>>;
                fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    f.write_str("images array")
                }
                fn visit_bool<E: de::Error>(self, _: bool) -> std::result::Result<Self::Value, E> {
                    Ok(None)
                }
                fn visit_i64<E: de::Error>(self, _: i64) -> std::result::Result<Self::Value, E> {
                    Ok(None)
                }
                fn visit_u64<E: de::Error>(self, _: u64) -> std::result::Result<Self::Value, E> {
                    Ok(None)
                }
                fn visit_f64<E: de::Error>(self, _: f64) -> std::result::Result<Self::Value, E> {
                    Ok(None)
                }
                fn visit_str<E: de::Error>(self, _: &str) -> std::result::Result<Self::Value, E> {
                    Ok(None)
                }
                fn visit_unit<E: de::Error>(self) -> std::result::Result<Self::Value, E> {
                    Ok(None)
                }
                fn visit_none<E: de::Error>(self) -> std::result::Result<Self::Value, E> {
                    Ok(None)
                }
                fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> std::result::Result<Self::Value, A::Error> {
                    while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
                    Ok(None)
                }
                fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<Self::Value, A::Error> {
                    let mut out = Vec::new();
                    while out.len() <= drawing::image::MAX_IMAGES {
                        let Some(item) = seq.next_element_seed(ImageItemSeed)? else { break };
                        out.push(item);
                    }
                    while seq.next_element::<IgnoredAny>()?.is_some() {}
                    Ok(Some(out))
                }
            }
            d.deserialize_any(V)
        }
    }

    struct RootVisitor;
    impl<'de> Visitor<'de> for RootVisitor {
        type Value = DrawingMeta;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("drawing JSON")
        }
        fn visit_bool<E: de::Error>(self, _: bool) -> std::result::Result<DrawingMeta, E> {
            Ok(DrawingMeta::default())
        }
        fn visit_i64<E: de::Error>(self, _: i64) -> std::result::Result<DrawingMeta, E> {
            Ok(DrawingMeta::default())
        }
        fn visit_u64<E: de::Error>(self, _: u64) -> std::result::Result<DrawingMeta, E> {
            Ok(DrawingMeta::default())
        }
        fn visit_f64<E: de::Error>(self, _: f64) -> std::result::Result<DrawingMeta, E> {
            Ok(DrawingMeta::default())
        }
        fn visit_str<E: de::Error>(self, _: &str) -> std::result::Result<DrawingMeta, E> {
            Ok(DrawingMeta::default())
        }
        fn visit_unit<E: de::Error>(self) -> std::result::Result<DrawingMeta, E> {
            Ok(DrawingMeta::default())
        }
        fn visit_none<E: de::Error>(self) -> std::result::Result<DrawingMeta, E> {
            Ok(DrawingMeta::default())
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<DrawingMeta, A::Error> {
            while seq.next_element::<IgnoredAny>()?.is_some() {}
            Ok(DrawingMeta::default())
        }
        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> std::result::Result<DrawingMeta, A::Error> {
            let mut meta = DrawingMeta::default();
            while let Some(key) = map.next_key::<String>()? {
                match key.as_str() {
                    "format" => meta.format = map.next_value_seed(OptStringSeed)?,
                    "format_pending" => meta.format_pending = map.next_value_seed(PendingSeed)?,
                    "name" => meta.name = map.next_value_seed(OptStringSeed)?,
                    "image_files" => meta.image_files = map.next_value_seed(ImagesSeed)?,
                    "images" => meta.images = map.next_value_seed(ImagesSeed)?,
                    _ => {
                        map.next_value::<IgnoredAny>()?;
                    }
                }
            }
            Ok(meta)
        }
    }

    let mut de = serde_json::Deserializer::from_slice(bytes);
    let meta = de.deserialize_any(RootVisitor)?;
    de.end()?;
    Ok(meta)
}

/// Stream-collect every JSON key or string value starting with `"blobs/"` without allocating a
/// `serde_json::Value` tree.
fn blob_names_in_json(bytes: &[u8]) -> Result<std::collections::BTreeSet<String>> {
    use serde::de::{self, DeserializeSeed, Deserializer, MapAccess, SeqAccess, Visitor};

    struct BlobSeed<'a>(&'a mut std::collections::BTreeSet<String>);

    impl<'de> DeserializeSeed<'de> for BlobSeed<'_> {
        type Value = ();
        fn deserialize<D: Deserializer<'de>>(self, d: D) -> std::result::Result<(), D::Error> {
            d.deserialize_any(self)
        }
    }

    impl<'de> Visitor<'de> for BlobSeed<'_> {
        type Value = ();
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("any valid JSON value")
        }
        fn visit_bool<E: de::Error>(self, _: bool) -> std::result::Result<(), E> {
            Ok(())
        }
        fn visit_i64<E: de::Error>(self, _: i64) -> std::result::Result<(), E> {
            Ok(())
        }
        fn visit_u64<E: de::Error>(self, _: u64) -> std::result::Result<(), E> {
            Ok(())
        }
        fn visit_f64<E: de::Error>(self, _: f64) -> std::result::Result<(), E> {
            Ok(())
        }
        fn visit_unit<E: de::Error>(self) -> std::result::Result<(), E> {
            Ok(())
        }
        fn visit_none<E: de::Error>(self) -> std::result::Result<(), E> {
            Ok(())
        }
        fn visit_str<E: de::Error>(self, v: &str) -> std::result::Result<(), E> {
            if v.starts_with("blobs/") {
                self.0.insert(v.to_owned());
            }
            Ok(())
        }
        fn visit_string<E: de::Error>(self, v: String) -> std::result::Result<(), E> {
            if v.starts_with("blobs/") {
                self.0.insert(v);
            }
            Ok(())
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<(), A::Error> {
            while seq.next_element_seed(BlobSeed(self.0))?.is_some() {}
            Ok(())
        }
        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> std::result::Result<(), A::Error> {
            while let Some(key) = map.next_key::<String>()? {
                if key.starts_with("blobs/") {
                    self.0.insert(key);
                }
                map.next_value_seed(BlobSeed(self.0))?;
            }
            Ok(())
        }
    }

    let mut names = std::collections::BTreeSet::new();
    let mut de = serde_json::Deserializer::from_slice(bytes);
    BlobSeed(&mut names).deserialize(&mut de).and_then(|()| de.end()).map_err(invalid)?;
    Ok(names)
}

/// The entry `name` kept as it came, with every container part its record names.
fn keep<R: Read + Seek>(reader: &mut PackageReader<R>, name: &str, bytes: &[u8], label: &str, format: u32, why: &IoError) -> Result<KeptDrawing> {
    let names = blob_names_in_json(bytes)?;
    let mut parts = vec![(name.to_string(), Arc::from(bytes))];
    for part in names {
        if !reader.has_part(&part) {
            continue;
        }
        let bytes = reader.limited_part(&part, super::part_limit(&part))?;
        parts.push((part, bytes.into()));
    }
    let note = format!("drawing `{label}` (saved by a newer SolidCraft, drawing format {format}, which this version can't read: {why})");
    Ok(KeptDrawing { parts, note })
}

#[cfg(test)]
#[path = "drawing_tests.rs"]
mod tests;
