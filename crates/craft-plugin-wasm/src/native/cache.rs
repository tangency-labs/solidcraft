//! Compiled components kept on disk between runs of the app (see [`crate::CompileCache`]).
//!
//! Wasmtime's own cache stores and loads the compiled code, so loading needs no unsafe
//! deserialization here. Each entry is a folder of its own, named after its key: Wasmtime's cache
//! for that one component, and `entry.json`, which records the key and the size and SHA-256 of
//! every file Wasmtime wrote. An entry is read only while all of that still matches. Anything else
//! (a damaged, truncated or replaced file, an extra file, a link, a missing or unreadable record)
//! is a miss, not an error: the folder is removed and the component compiled again, which writes
//! the entry anew. When the folder can't be removed, the component is compiled without the cache,
//! so what was rejected is never loaded. The record's modification time is the entry's last use, and the least recently
//! used entries go first when the cache is over its size.

use super::trap;
use crate::CompileCache;
use craft_plugin::Result;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use wasmtime::component::Component;
use wasmtime::{Cache, CacheConfig, Config, Engine};

/// Changes when an entry's layout does; entries of another layout are then never read.
pub(super) const LAYOUT: &str = "v1";
pub(super) const RECORD: &str = "entry.json";
const RECORD_TEMP: &str = "entry.json.new";
const MAX_RECORD_BYTES: u64 = 64 * 1024;
/// Wasmtime writes one file per component; a folder with more was not written by it.
const MAX_FILES: usize = 16;
const MAX_DEPTH: usize = 4;

/// Where a compiled component came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Source {
    Compiled,
    Disk,
}

struct Key {
    /// SHA-256 of the component's bytes.
    component: String,
    /// SHA-256 of everything about the runtime that changes the compiled code.
    engine: String,
}

impl Key {
    fn folder(&self) -> String {
        hex(&Sha256::digest(format!("{}:{}", self.component, self.engine)))
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Feeds a [`std::hash::Hash`] into SHA-256, which, unlike `DefaultHasher`, is the same in every
/// process and every build.
struct Sha256Hasher(Sha256);

impl std::hash::Hasher for Sha256Hasher {
    /// Unused: only the digest is read.
    fn finish(&self) -> u64 {
        0
    }
    fn write(&mut self, bytes: &[u8]) {
        self.0.update(bytes);
    }
}

/// What makes compiled code differ: Wasmtime's version, its target, CPU features and settings (fuel,
/// interruption, Wasm features: its own compatibility hash), and the stack the app asked for.
pub(super) fn engine_key(engine: &Engine, stack_bytes: usize) -> String {
    use std::hash::Hash;
    let mut hasher = Sha256Hasher(Sha256::new());
    hasher.0.update(concat!("wasmtime ", env!("CRAFT_WASMTIME_VERSION"), "; craft-plugin-wasm ", env!("CARGO_PKG_VERSION"), ";"));
    engine.precompile_compatibility_hash().hash(&mut hasher);
    hasher.0.update((stack_bytes as u64).to_le_bytes());
    hex(&hasher.0.finalize())
}

/// Compiles `bytes` with `config`, or loads the entry in `cache` that an earlier run wrote.
pub(super) fn compile(config: &Config, bytes: &[u8], stack_bytes: usize, cache: &CompileCache) -> Result<(Engine, Component, Source)> {
    let plain = Engine::new(config).map_err(|e| trap(e.to_string()))?;
    let key = Key { component: hex(&Sha256::digest(bytes)), engine: engine_key(&plain, stack_bytes) };
    let root = cache.dir.join(LAYOUT);
    let entry = root.join(key.folder());
    let verified = verify(&entry, &key);
    if !verified && !remove(&entry) {
        // The rejected files are still there, and Wasmtime's cache reads whatever it finds: compile
        // without the cache, so they are never loaded.
        let component = from_binary(&plain, bytes)?;
        return Ok((plain, component, Source::Compiled));
    }
    let Some(store) = open(&cache.dir, &entry) else {
        // The folder can't be written: compile as if there were no cache.
        let component = from_binary(&plain, bytes)?;
        return Ok((plain, component, Source::Compiled));
    };
    let mut config = config.clone();
    config.cache(Some(store.clone()));
    let engine = Engine::new(&config).map_err(|e| trap(e.to_string()))?;
    let component = from_binary(&engine, bytes)?;
    if verified && store.cache_hits() > 0 && store.cache_misses() == 0 {
        touch(&entry);
        return Ok((engine, component, Source::Disk));
    }
    if store.cache_misses() > 0 {
        if write_record(&entry, &key).is_none() {
            // A failed removal leaves an entry without a record, which the next load rejects.
            let _ = remove(&entry);
        }
        evict(&root, cache.max_bytes, &entry);
    }
    Ok((engine, component, Source::Compiled))
}

pub(super) fn from_binary(engine: &Engine, bytes: &[u8]) -> Result<Component> {
    Component::from_binary(engine, bytes).map_err(|e| {
        // Inspect the parser error itself, not plugin names in the compilation context.
        if let Some(error) = e.downcast_ref::<wasmtime::wasmparser::BinaryReaderError>() {
            return super::component_parse_error(error);
        }
        trap(format!("component compilation failed: {e:#}"))
    })
}

/// Wasmtime's cache for one entry. Its own clean-up and recompression are off: they would change
/// the files the record vouches for, and the size limit is kept here, across entries.
fn open(dir: &Path, entry: &Path) -> Option<Cache> {
    fs::create_dir_all(entry).map_err(|e| log::warn!("plugin compile cache: {}: {e}", entry.display())).ok()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o700));
    }
    #[cfg(not(unix))]
    let _ = dir;
    let mut config = CacheConfig::new();
    config
        .with_directory(entry.join("wasmtime"))
        .with_file_count_soft_limit(1 << 20)
        .with_files_total_size_soft_limit(1 << 50)
        .with_optimized_compression_usage_counter_threshold(u64::MAX);
    Cache::new(config).map_err(|e| log::warn!("plugin compile cache: {e:#}")).ok()
}

/// The files under `dir` that Wasmtime reads back, as (path relative to `dir`, path, size), or
/// `None` when something there is not a plain file or folder, or there is too much of it. Wasmtime's
/// usage counts (`.stats`) and half-written files (`.wip-`) are never read as code and are skipped.
fn files(dir: &Path) -> Option<Vec<(String, PathBuf, u64)>> {
    let mut found = Vec::new();
    let mut folders = vec![(dir.to_path_buf(), String::new(), 0usize)];
    while let Some((folder, prefix, depth)) = folders.pop() {
        for item in fs::read_dir(&folder).ok()? {
            let item = item.ok()?;
            let name = item.file_name().into_string().ok()?;
            let relative = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
            // `file_type` does not follow links, so a link is neither a file nor a folder here.
            let kind = item.file_type().ok()?;
            if kind.is_dir() {
                if depth >= MAX_DEPTH {
                    return None;
                }
                folders.push((item.path(), relative, depth + 1));
            } else if kind.is_file() {
                if (depth == 0 && (name == RECORD || name == RECORD_TEMP)) || name.ends_with(".stats") || name.contains(".wip-") {
                    continue;
                }
                found.push((relative, item.path(), item.metadata().ok()?.len()));
                if found.len() > MAX_FILES {
                    return None;
                }
            } else {
                return None;
            }
        }
    }
    Some(found)
}

fn sha256_file(path: &Path) -> Option<String> {
    let mut file = fs::File::open(path).ok()?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        let read = file.read(&mut buffer).ok()?;
        if read == 0 {
            return Some(hex(&hasher.finalize()));
        }
        hasher.update(buffer.get(..read)?);
    }
}

/// Whether `entry` holds exactly what its record says, for this key.
fn verify(entry: &Path, key: &Key) -> bool {
    matches_record(entry, key).is_some()
}

fn matches_record(entry: &Path, key: &Key) -> Option<()> {
    let record_path = entry.join(RECORD);
    let metadata = fs::symlink_metadata(&record_path).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_RECORD_BYTES {
        return None;
    }
    let record: Value = serde_json::from_slice(&fs::read(&record_path).ok()?).ok()?;
    if record.get("layout")?.as_str()? != LAYOUT
        || record.get("component")?.as_str()? != key.component
        || record.get("engine")?.as_str()? != key.engine
    {
        return None;
    }
    let mut expected = BTreeMap::new();
    for file in record.get("files")?.as_array()? {
        expected.insert(file.get("path")?.as_str()?, (file.get("size")?.as_u64()?, file.get("sha256")?.as_str()?));
    }
    let found = files(entry)?;
    if expected.is_empty() || found.len() != expected.len() {
        return None;
    }
    for (relative, path, size) in found {
        let (want_size, want_sha) = expected.get(relative.as_str())?;
        if *want_size != size || sha256_file(&path)?.as_str() != *want_sha {
            return None;
        }
    }
    Some(())
}

/// Records what Wasmtime just wrote in `entry`, replacing the record in one step.
fn write_record(entry: &Path, key: &Key) -> Option<()> {
    let mut listed = Vec::new();
    for (relative, path, size) in files(entry)? {
        listed.push(json!({"path": relative, "size": size, "sha256": sha256_file(&path)?}));
    }
    if listed.is_empty() {
        return None;
    }
    let record = json!({"layout": LAYOUT, "component": key.component, "engine": key.engine, "files": listed});
    let temp = entry.join(RECORD_TEMP);
    fs::write(&temp, serde_json::to_vec(&record).ok()?).ok()?;
    fs::rename(&temp, entry.join(RECORD)).ok()
}

/// Marks `entry` as just used.
fn touch(entry: &Path) {
    if let Ok(file) = fs::File::options().write(true).open(entry.join(RECORD)) {
        let _ = file.set_modified(SystemTime::now());
    }
}

/// Removes `entry`; whether it is gone.
fn remove(entry: &Path) -> bool {
    match fs::remove_dir_all(entry) {
        Ok(()) => true,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => true,
        Err(e) => {
            log::warn!("plugin compile cache: could not remove {}: {e}", entry.display());
            false
        }
    }
}

/// The bytes found under a folder, and whether every part of it could be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Scan {
    bytes: u64,
    complete: bool,
}

/// Every byte under `dir`, links not followed. A part that can't be read makes the scan incomplete,
/// never zero bytes.
fn size(dir: &Path, depth: usize) -> Scan {
    let mut scan = Scan { bytes: 0, complete: true };
    let Ok(items) = fs::read_dir(dir) else {
        scan.complete = false;
        return scan;
    };
    for item in items {
        let Ok(item) = item else {
            scan.complete = false;
            continue;
        };
        match item.file_type() {
            Ok(kind) if kind.is_dir() => {
                if depth >= MAX_DEPTH {
                    scan.complete = false;
                    continue;
                }
                let inner = size(&item.path(), depth + 1);
                scan.bytes = scan.bytes.saturating_add(inner.bytes);
                scan.complete &= inner.complete;
            }
            Ok(kind) if kind.is_file() => match item.metadata() {
                Ok(metadata) => scan.bytes = scan.bytes.saturating_add(metadata.len()),
                Err(_) => scan.complete = false,
            },
            // A link counts its own few bytes at most; it is not followed.
            Ok(_) => {}
            Err(_) => scan.complete = false,
        }
    }
    scan
}

/// Removes the least recently used entries until the cache fits in `max_bytes`. `keep`, the entry
/// just written, stays even when it alone is larger; the next entry written then replaces it.
///
/// An entry that can't be fully read can't be loaded either, and its unread bytes are unknown, so it
/// goes first, and while one is left the cache is not known to fit: the other entries go too. An
/// entry's bytes count as freed only once it is removed; when a removal fails, what is left of it is
/// still counted and the next entry is tried.
fn evict(root: &Path, max_bytes: u64, keep: &Path) {
    let Ok(items) = fs::read_dir(root) else { return };
    let mut entries: Vec<(bool, SystemTime, Scan, PathBuf)> = items
        .flatten()
        .filter(|item| item.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|item| {
            let path = item.path();
            let used = fs::metadata(path.join(RECORD)).or_else(|_| item.metadata()).and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH);
            let scan = size(&path, 0);
            (scan.complete, used, scan, path)
        })
        .collect();
    let mut total = entries.iter().map(|(_, _, scan, _)| scan.bytes).fold(0u64, u64::saturating_add);
    let mut unknown = entries.iter().filter(|(complete, _, _, path)| !complete && path != keep).count();
    // Incomplete entries first (`false` sorts before `true`), then the least recently used.
    entries.sort_by_key(|(complete, used, _, _)| (*complete, *used));
    for (complete, _, scan, path) in entries {
        if total <= max_bytes && unknown == 0 {
            break;
        }
        if path == keep {
            continue;
        }
        if remove(&path) {
            total = total.saturating_sub(scan.bytes);
            if !complete {
                unknown = unknown.saturating_sub(1);
            }
        } else {
            // Part of it may be gone: credit only what a complete scan shows was freed.
            let left = size(&path, 0);
            if left.complete && complete {
                total = total.saturating_sub(scan.bytes.saturating_sub(left.bytes));
            }
        }
    }
}
