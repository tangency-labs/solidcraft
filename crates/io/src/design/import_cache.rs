//! The import cache: the STEP imports a design's Import features read, saved with it so a release
//! that evaluates the design again (its timeline's code changed) need not read its STEP files
//! again (#9456). Each import is keyed by the SHA-256 of its STEP text and kept in
//! `cache/imports.json` (the records) and `cache/imports.bin` (the bodies, in SolidCraft's exact
//! native B-rep encoding); the manifest's key ties them to the source that reads STEP files
//! (`SOLIDCRAFT_IMPORT_SOURCE`, build.rs) and to the platform (docs/design-container.md).
use super::*;
use solidcraft_doc::{ImportStore, SavedImports};
use solidcraft_kernel::{ImportedBody, StepImport, StepNode};
use std::collections::HashMap;

const FORMAT: &str = "solidcraft-import-cache/1";
const MANIFEST: &str = "cache/imports.json";
const PAYLOAD: &str = "cache/imports.bin";
/// Imports in one design, at most.
const MAX_IMPORTS: usize = 1_000;
/// Bodies of one import (the importer reads at most 10,000).
const MAX_BODIES: usize = 10_000;
/// Stored bodies of every import together.
const MAX_SLICES: usize = 100_000;
/// Face entities recorded for one body.
const MAX_FACES: usize = 1_000_000;
const MAX_MANIFEST: usize = 32 << 20;

/// What reads a STEP file: the same source (build.rs), compiled for the same platform, reads the
/// same import. The application version and the commit are recorded, not compared.
fn importer() -> String {
    format!("{}/{}-{}", env!("SOLIDCRAFT_IMPORT_SOURCE"), std::env::consts::ARCH, std::env::consts::OS)
}

/// The imports a design file keeps, as stored: an import is decoded only when a feature asks for
/// it, and a save copies the others as they are.
#[derive(Debug)]
struct Stored {
    /// Each import's record, by the digest of its text.
    records: HashMap<String, Value>,
    /// Each stored body's bytes, in `payload`.
    slices: Vec<(usize, usize)>,
    payload: Vec<u8>,
}

impl Stored {
    fn slice(&self, i: &Value) -> Option<&[u8]> {
        let (offset, length) = *self.slices.get(usize::try_from(i.as_u64()?).ok()?)?;
        self.payload.get(offset..offset.checked_add(length)?)
    }
}

impl ImportStore for Stored {
    fn has(&self, digest: &str) -> bool {
        self.records.contains_key(digest)
    }

    fn import(&self, digest: &str) -> Option<StepImport> {
        let record = self.records.get(digest)?;
        let entries = record.get("bodies")?.as_array()?;
        let slices: Vec<&[u8]> = entries.iter().map(|b| self.slice(b.get("slice")?)).collect::<Option<_>>()?;
        let decoded = solidcraft_kernel::map_on_threads(&slices, 16, |bytes| solidcraft_kernel::decode_native_body(bytes).ok());
        let mut bodies = Vec::with_capacity(entries.len());
        for (entry, body) in entries.iter().zip(decoded) {
            let text = |key: &str| entry.get(key).and_then(Value::as_str).map(str::to_string);
            let face_origin: Vec<u64> = serde_json::from_value(entry.get("face_origin")?.clone()).ok()?;
            let path: Vec<String> = serde_json::from_value(entry.get("path")?.clone()).ok()?;
            let color: Option<[f32; 3]> = serde_json::from_value(entry.get("color")?.clone()).ok()?;
            if face_origin.len() > MAX_FACES || path.len() > MAX_FACES || color.is_some_and(|c| c.iter().any(|x| !x.is_finite())) {
                return None;
            }
            bodies.push(ImportedBody {
                source_item: entry.get("source_item")?.as_u64()?,
                face_origin,
                name: text("name")?,
                body: body.flatten()?,
                color,
                path,
                instance: match entry.get("instance")? {
                    Value::Null => None,
                    v => Some(v.as_str()?.to_string()),
                },
                closed: entry.get("closed")?.as_bool()?,
                file_faces: usize::try_from(entry.get("file_faces")?.as_u64()?).ok()?,
            });
        }
        let tree: Vec<StepNode> = serde_json::from_value(record.get("tree")?.clone()).ok()?;
        if !tree.iter().all(|n| indices_within(n, bodies.len(), 0)) {
            return None;
        }
        let warnings: Vec<String> = serde_json::from_value(record.get("warnings")?.clone()).ok()?;
        let text = |key: &str| record.get(key).and_then(Value::as_str).map(str::to_string);
        Some(StepImport {
            bodies,
            tree,
            warnings,
            schema: text("schema")?,
            system: text("system")?,
            preprocessor: text("preprocessor")?,
            description: text("description")?,
        })
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Whether every body index of `node` and its children is below `bodies`, its placement finite.
fn indices_within(node: &StepNode, bodies: usize, depth: usize) -> bool {
    depth < 256
        && node.transform.iter().flatten().all(|x| x.is_finite())
        && node.bodies.iter().all(|b| *b < bodies)
        && node.children.iter().all(|c| indices_within(c, bodies, depth + 1))
}

/// An import's record (its bodies' slices numbered from `first`) and its bodies' encodings, or
/// `None` when a body does not decode to itself (then it is not kept, and is read again).
fn encode_import(imp: &StepImport, first: usize) -> Option<(Value, Vec<Vec<u8>>)> {
    if imp.bodies.len() > MAX_BODIES {
        return None;
    }
    let bodies: Vec<&solidcraft_kernel::Body> = imp.bodies.iter().map(|b| &b.body).collect();
    let encoded = solidcraft_kernel::map_on_threads(&bodies, 16, |b| {
        let bytes = solidcraft_kernel::encode_native_body(b).ok()?;
        let back = solidcraft_kernel::decode_native_body(&bytes).ok()?;
        let counts = |x: &solidcraft_kernel::Body| (x.face_count(), x.edge_count(), x.vertex_count(), x.shell_count());
        (counts(b) == counts(&back)).then_some(bytes)
    });
    let slices: Vec<Vec<u8>> = encoded.into_iter().collect::<Option<Option<Vec<_>>>>()??;
    let entries: Vec<Value> = imp
        .bodies
        .iter()
        .enumerate()
        .map(|(k, b)| {
            json!({
                "slice": first + k, "source_item": b.source_item, "face_origin": b.face_origin, "name": b.name, "color": b.color,
                "path": b.path, "instance": b.instance, "closed": b.closed, "file_faces": b.file_faces,
            })
        })
        .collect();
    let record = json!({
        "bodies": entries, "tree": imp.tree, "warnings": imp.warnings, "schema": imp.schema, "system": imp.system,
        "preprocessor": imp.preprocessor, "description": imp.description,
    });
    Some((record, slices))
}

/// Encode the imports a save keeps in `space` bytes (manifest and payload together): each one
/// read in this session is encoded, each one the file it was opened from kept is copied as it
/// was. An import that does not fit is left out; `None` when none is kept.
pub(super) fn encode_imports(saved: &SavedImports, space: usize) -> Option<(Vec<u8>, Vec<u8>)> {
    let stored = saved.stored.as_ref().and_then(|s| s.as_any().downcast_ref::<Stored>());
    let mut imports = Map::new();
    let mut slices = Vec::new();
    let mut data = Vec::new();
    for digest in saved.digests.iter().take(MAX_IMPORTS) {
        let (mut record, bytes) = if let Some(imp) = saved.read.get(digest) {
            let Some(encoded) = encode_import(imp, slices.len()) else { continue };
            encoded
        } else if let Some((record, stored)) = stored.and_then(|s| Some((s.records.get(digest)?, s))) {
            let Some(entries) = record.get("bodies").and_then(Value::as_array) else { continue };
            let Some(bytes) = entries.iter().map(|b| stored.slice(b.get("slice")?).map(<[u8]>::to_vec)).collect::<Option<Vec<_>>>() else { continue };
            (record.clone(), bytes)
        } else {
            continue;
        };
        let size = bytes.iter().fold(0usize, |n, b| n.saturating_add(b.len()));
        if slices.len().saturating_add(bytes.len()) > MAX_SLICES || data.len().saturating_add(size) > space {
            continue;
        }
        // Renumber the bodies' slices to where they go now.
        if let Some(entries) = record.get_mut("bodies").and_then(Value::as_array_mut) {
            for (k, entry) in entries.iter_mut().enumerate() {
                entry["slice"] = json!(slices.len() + k);
            }
        }
        for b in bytes {
            slices.push(json!([data.len(), b.len()]));
            data.extend_from_slice(&b);
        }
        imports.insert(digest.clone(), record);
    }
    if imports.is_empty() {
        return None;
    }
    let manifest = serde_json::to_vec(&json!({
        "format": FORMAT, "producer": native_cache::PRODUCER, "app": env!("CARGO_PKG_VERSION"), "build": native_cache::build(), "importer": importer(),
        "payload": digest(&data), "slices": slices, "imports": imports,
    }))
    .ok()?;
    (manifest.len() <= MAX_MANIFEST && manifest.len().saturating_add(data.len()) <= space).then_some((manifest, data))
}

/// The imports a design file keeps, when this build's importer read them. Any failure is a miss,
/// never a document error; the bodies are decoded only when a feature asks for one.
pub(super) fn read_imports_with<R: Read + Seek>(reader: &mut PackageReader<R>) -> Result<SavedImports> {
    let manifest: Value = serde_json::from_slice(&reader.limited_part(MANIFEST, MAX_MANIFEST)?).map_err(invalid)?;
    let is = |key: &str, value: &str| manifest.get(key).and_then(Value::as_str) == Some(value);
    if !(is("format", FORMAT)
        && is("producer", native_cache::PRODUCER)
        && is("importer", &importer())
        && native_cache::source_trusted(env!("SOLIDCRAFT_IMPORT_SOURCE"), cfg!(any(test, feature = "trust-unknown-source"))))
    {
        return Err(invalid("import cache key"));
    }
    let payload = reader.limited_part(PAYLOAD, max_design())?;
    if !is("payload", &digest(&payload)) {
        return Err(invalid("import cache payload key"));
    }
    let count = |v: Option<&Value>| v.and_then(Value::as_u64).and_then(|n| usize::try_from(n).ok()).ok_or_else(|| invalid("import cache number"));
    let listed = manifest.get("slices").and_then(Value::as_array).filter(|s| s.len() <= MAX_SLICES).ok_or_else(|| invalid("import cache slices"))?;
    let mut end = 0usize;
    let mut slices = Vec::with_capacity(listed.len());
    for s in listed {
        let (offset, length) = (count(s.get(0))?, count(s.get(1))?);
        if offset != end {
            return Err(invalid("overlapping cached imports"));
        }
        end = end.checked_add(length).ok_or_else(|| invalid("cached import length"))?;
        slices.push((offset, length));
    }
    if end != payload.len() {
        return Err(invalid("trailing cached import data"));
    }
    let imports = manifest.get("imports").and_then(Value::as_object).filter(|i| i.len() <= MAX_IMPORTS).ok_or_else(|| invalid("cached imports"))?;
    let mut records = HashMap::with_capacity(imports.len());
    for (digest, record) in imports {
        let bodies = record.get("bodies").and_then(Value::as_array).filter(|b| b.len() <= MAX_BODIES).ok_or_else(|| invalid("cached import"))?;
        if digest.len() != 64 || bodies.iter().any(|b| b.get("slice").and_then(Value::as_u64).is_none_or(|i| i as usize >= slices.len())) {
            return Err(invalid("cached import"));
        }
        records.insert(digest.clone(), record.clone());
    }
    let stored: Arc<dyn ImportStore> = Arc::new(Stored { records, slices, payload });
    Ok(SavedImports { stored: Some(stored), ..SavedImports::default() })
}

pub(super) const ENTRIES: [&str; 2] = [MANIFEST, PAYLOAD];
