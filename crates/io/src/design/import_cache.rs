//! Local copies of untouched imports, using the saved-design model cache and its trust rules.
//! This is disposable application data, not a design-format extension.
use super::*;
use std::path::{Path, PathBuf};

const BINDING: &str = "cache/import.json";
const MAX_BINDING: usize = 1 << 20;
const MAX_ENTRIES: usize = 500;
const IMPORT_CACHE_BYTES: u64 = 512 << 20;
static WRITER: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// One bounded store across application updates. Old builds' entries are evicted along with
/// current ones, but the normal model reader never adopts another build's evaluation.
pub struct ImportCache {
    directory: PathBuf,
    pub(super) budget: u64,
}

impl ImportCache {
    pub fn new(directory: PathBuf) -> Self {
        Self { directory, budget: IMPORT_CACHE_BYTES }
    }

    /// Exact source bytes, file name (which affects component names and format detection),
    /// and import options, plus the same producer identity as the saved-design cache.
    pub fn key(bytes: &[u8], file: &str, options: &Value) -> String {
        digest(
            json!([digest(bytes), Path::new(file).file_name().map(|f| f.to_string_lossy()), options, native_cache::identity()])
                .to_string()
                .as_bytes(),
        )
    }

    fn path(&self, key: &str) -> Option<PathBuf> {
        (key.len() == 64 && key.bytes().all(|b| b.is_ascii_hexdigit())).then(|| self.directory.join(format!("{key}.sc")))
    }

    /// Any absent, incompatible or damaged entry is a miss, before the caller changes a design.
    pub fn read(&self, key: &str) -> Option<(Document, solidcraft_doc::SavedModel, Value)> {
        let path = self.path(key)?;
        if !std::fs::symlink_metadata(&path).ok()?.file_type().is_file() {
            return None;
        }
        let file = std::fs::File::open(path).ok()?;
        let metadata = file.metadata().ok()?;
        if !metadata.is_file() || metadata.len() > self.budget.min(max_design() as u64) {
            return None;
        }
        let mut reader = DesignContainerReader::new(file).ok()?;
        let binding: Value = serde_json::from_slice(&reader.reader.limited_part(BINDING, MAX_BINDING).ok()?).ok()?;
        if binding.get("key").and_then(Value::as_str) != Some(key) {
            return None;
        }
        let model = reader.geometry()?;
        let doc = reader.read_document().ok()?;
        // No partial acceptance: feature fingerprints and every saved state must match too.
        solidcraft_doc::Model::adopt(&doc, &model).ok()?;
        let reply = binding.get("reply")?.as_object()?.clone();
        Some((doc, model, Value::Object(reply)))
    }

    /// Called only immediately after import, before any stand-up answer or subsequent edit.
    /// If the normal writer cannot keep a model, leave no entry that would re-import on a hit.
    pub fn write(&self, key: &str, doc: &Document, model: &solidcraft_doc::SavedModel, reply: &Value) -> Result<()> {
        let path = self.path(key).ok_or_else(|| invalid("import cache key"))?;
        let bytes = write_design_container_with_cache(doc, DesignCompression::Fast, None, Some(model))?;
        let mut reader = DesignContainerReader::new(Cursor::new(&bytes))?;
        if reader.reader.limited_part(native_cache::ENTRIES[0], 32 << 20).is_err() {
            return Err(invalid("import model was not stored"));
        }
        let binding = serde_json::to_vec(&json!({"key": key, "reply": reply})).map_err(invalid)?;
        if binding.len() > MAX_BINDING {
            return Err(invalid("import reply size"));
        }
        let mut zip = zip::ZipWriter::new_append(Cursor::new(bytes)).map_err(invalid)?;
        zip.start_file(BINDING, zip::write::SimpleFileOptions::default()).map_err(invalid)?;
        zip.write_all(&binding).map_err(invalid)?;
        let bytes = zip.finish().map_err(invalid)?.into_inner();
        if bytes.len() as u64 > self.budget.min(max_design() as u64) {
            return Err(invalid("import cache size"));
        }
        std::fs::create_dir_all(&self.directory).map_err(invalid)?;
        // Serialize eviction and writes within this process. Atomic replacement also keeps
        // concurrent application instances from reading an incomplete entry.
        let _writer = WRITER.lock().map_err(|_| invalid("import cache lock"))?;
        // Design saves deliberately follow links; disposable cache entries must never replace
        // a linked file outside this store.
        if std::fs::symlink_metadata(&path).is_ok_and(|metadata| !metadata.file_type().is_file()) {
            return Err(invalid("import cache entry is not a regular file"));
        }
        self.prune(bytes.len() as u64, MAX_ENTRIES - 1)?;
        crate::write_atomic(&path, &bytes, false).map_err(invalid)?;
        self.prune(0, MAX_ENTRIES)
    }

    fn entries(&self) -> Result<Vec<(std::time::SystemTime, PathBuf, u64)>> {
        let mut entries = Vec::new();
        let dir = match std::fs::read_dir(&self.directory) {
            Ok(dir) => dir,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(entries),
            Err(e) => return Err(invalid(e)),
        };
        for entry in dir {
            let entry = entry.map_err(invalid)?;
            let path = entry.path();
            let managed = path.file_stem().and_then(|name| name.to_str()).and_then(|key| self.path(key)).is_some();
            if managed && entry.file_type().map_err(invalid)?.is_file() && path.extension().is_some_and(|e| e == "sc") {
                let meta = entry.metadata().map_err(invalid)?;
                entries.push((meta.modified().map_err(invalid)?, path, meta.len()));
            }
        }
        entries.sort();
        Ok(entries)
    }

    fn prune(&self, incoming: u64, count: usize) -> Result<()> {
        let entries = self.entries()?;
        let mut size = entries.iter().fold(incoming, |n, (_, _, len)| n.saturating_add(*len));
        let mut left = entries.len();
        for (_, path, len) in entries {
            if size <= self.budget && left <= count {
                break;
            }
            std::fs::remove_file(path).map_err(invalid)?;
            size = size.saturating_sub(len);
            left = left.saturating_sub(1);
        }
        Ok(())
    }

    pub fn clear(&self) -> Result<usize> {
        let _writer = WRITER.lock().map_err(|_| invalid("import cache lock"))?;
        let entries = self.entries()?;
        for (_, path, _) in &entries {
            std::fs::remove_file(path).map_err(invalid)?;
        }
        Ok(entries.len())
    }
}
