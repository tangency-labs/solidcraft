//! The model store: a per-user copy of the model cache of every design saved on this machine, so
//! a design whose file holds no model this build can use (saved without it, its model left out
//! because it did not fit, or an earlier version of it brought back by version control) opens
//! without being evaluated again, when this machine saved that exact document before (#19123).
//! Disposable application data in the platform's cache folder, not a design-format extension:
//! an entry is the file's own `container.json` and model cache, copied as they were written, and
//! is read under the same rules as the cache in the file ([`native_cache`]).
use super::*;
use std::path::{Path, PathBuf};

/// The most bytes the store keeps; the entries used longest ago go first.
const STORE_BYTES: u64 = 2 << 30;
/// The most entries the store keeps.
const MAX_STORED: usize = 2000;
/// The parts an entry holds: the container's header and its model cache.
const PARTS: [&str; 3] = ["container.json", native_cache::ENTRIES[0], native_cache::ENTRIES[1]];
static WRITER: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// One bounded store of saved models, keyed by the document they were evaluated for and the
/// evaluation source, platform and kernel that computed them ([`native_cache::store_key`]).
#[derive(Clone)]
pub struct ModelStore {
    directory: PathBuf,
    pub(super) budget: u64,
}

impl ModelStore {
    pub fn new(directory: PathBuf) -> Self {
        Self { directory, budget: STORE_BYTES }
    }

    fn path(&self, key: &str) -> Option<PathBuf> {
        (key.len() == 64 && key.bytes().all(|b| b.is_ascii_hexdigit())).then(|| self.directory.join(format!("{key}.scm")))
    }

    /// Keep the model cache of the design container just saved (`container`, its bytes). A
    /// container without one is left out. The parts are copied as written, not encoded or
    /// compressed again.
    pub fn keep(&self, container: &[u8]) -> Result<()> {
        let mut source = zip::ZipArchive::new(Cursor::new(container)).map_err(invalid)?;
        if PARTS.iter().any(|part| source.index_for_name(part).is_none()) {
            return Ok(());
        }
        let document = {
            let mut reader = PackageReader::new(Cursor::new(container))?;
            reader.document(16 << 20)?
        };
        let path = self.path(&native_cache::store_key(&document)).ok_or_else(|| invalid("model store key"))?;
        let mut entry = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for part in PARTS {
            let index = source.index_for_name(part).ok_or_else(|| invalid("model store part"))?;
            entry.raw_copy_file(source.by_index_raw(index).map_err(invalid)?).map_err(invalid)?;
        }
        let bytes = entry.finish().map_err(invalid)?.into_inner();
        if bytes.len() as u64 > self.budget.min(max_design() as u64) {
            return Err(invalid("model store entry size"));
        }
        std::fs::create_dir_all(&self.directory).map_err(invalid)?;
        // Writes and eviction take turns within this process; replacing an entry atomically keeps
        // another instance of the application from reading half of one.
        let _writer = WRITER.lock().map_err(|_| invalid("model store lock"))?;
        self.prune(bytes.len() as u64, MAX_STORED - 1)?;
        crate::safe_write::write_atomic_no_follow(&path, &bytes).map_err(invalid)?;
        Ok(())
    }

    /// The model this machine saved for exactly `document` (a design file's `document.json`, as
    /// read), when this build's evaluation source computed it. An absent, other or damaged entry is
    /// a miss. With `validate` false the document is not validated here: the caller reads it
    /// next, which fails when it is not valid ([`native_cache::read_geometry_before_document`]).
    pub(super) fn read(&self, document: &[u8], validate: bool) -> Option<solidcraft_doc::SavedModel> {
        let path = self.path(&native_cache::store_key(document))?;
        if !std::fs::symlink_metadata(&path).ok()?.file_type().is_file() {
            return None;
        }
        let file = std::fs::File::open(&path).ok()?;
        let metadata = file.metadata().ok()?;
        if !metadata.is_file() || metadata.len() > self.budget.min(max_design() as u64) {
            return None;
        }
        let mut reader = PackageReader::new(&file).ok()?;
        let model = if validate {
            native_cache::read_geometry_with(&mut reader, document)
        } else {
            native_cache::read_geometry_before_document(&mut reader, document)
        }
        .ok()?;
        // Used now: the entries used longest ago are evicted first.
        let _ = file.set_modified(std::time::SystemTime::now());
        Some(model)
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
            if self.managed(&path) && entry.file_type().map_err(invalid)?.is_file() {
                let meta = entry.metadata().map_err(invalid)?;
                entries.push((meta.modified().map_err(invalid)?, path, meta.len()));
            }
        }
        entries.sort();
        Ok(entries)
    }

    /// Whether `path` names an entry of this store (nothing else in its folder is touched).
    fn managed(&self, path: &Path) -> bool {
        path.extension().is_some_and(|e| e == "scm") && path.file_stem().and_then(|name| name.to_str()).and_then(|key| self.path(key)).is_some()
    }

    /// Evict the entries used longest ago until `incoming` more bytes fit the budget and at most
    /// `count` entries are left.
    fn prune(&self, incoming: u64, count: usize) -> Result<()> {
        let entries = self.entries()?;
        let mut size = entries.iter().fold(incoming, |n, (_, _, len)| n.saturating_add(*len));
        let mut left = entries.len();
        for (_, path, len) in entries {
            if size <= self.budget && left <= count {
                break;
            }
            // Another instance may have evicted it already.
            match std::fs::remove_file(path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(invalid(e)),
            }
            size = size.saturating_sub(len);
            left = left.saturating_sub(1);
        }
        Ok(())
    }

    /// Remove every entry; says how many there were.
    pub fn clear(&self) -> Result<usize> {
        let _writer = WRITER.lock().map_err(|_| invalid("model store lock"))?;
        let entries = self.entries()?;
        for (_, path, _) in &entries {
            std::fs::remove_file(path).map_err(invalid)?;
        }
        Ok(entries.len())
    }

    /// The bytes the store holds.
    pub fn size(&self) -> Result<u64> {
        Ok(self.entries()?.iter().map(|(_, _, len)| len).sum())
    }
}
