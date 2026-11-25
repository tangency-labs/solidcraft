//! Approved plugin grants kept between sessions. The desktop app, the command line and headless
//! MCP share one store: a hidden `.grants` folder in the installed plugins' folder, with one
//! `<plugin id>.json` record per plugin. A record is bound to the exact manifest and component the
//! user approved (`Grants::package_hash`), so a changed package needs approval again.
//!
//! Records are written whole, to a hidden temporary file renamed into place, so another program
//! reading the store sees the old record or the new one. A record that does not read, is too
//! large, names no valid plugin id or holds more than an installation can approve is ignored: the
//! plugin then runs with nothing granted.

use super::{COMMAND_GROUPS, Grants, MAX_APPROVED_FUEL, MAX_MEMORY_MIB, MAX_PICKED_FOLDERS, valid_id};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// The store's folder name inside the plugins folder. Hidden: plugin discovery skips it.
pub const FOLDER: &str = ".grants";
/// The most records read, as many as preferences keep.
pub const MAX_RECORDS: usize = 256;
/// The largest record read or written: room for every folder `valid_record` accepts at its
/// longest path, escaped (160 folders of 4096 bytes).
const MAX_RECORD_BYTES: u64 = 2 << 20;
/// The most folder entries looked at while reading the store.
const MAX_ENTRIES: usize = 1024;

/// The store's folder for a plugins folder.
pub fn folder(plugins: &Path) -> PathBuf {
    plugins.join(FOLDER)
}

/// Whether `grants` is a record an installation could have made: known command groups, memory
/// within the Wasm32 limit, fuel within the approval ceiling and at most 32 distinct, usable
/// absolute folders.
pub fn valid_record(grants: &Grants) -> bool {
    let mut names = BTreeSet::new();
    grants.package_hash.as_deref().is_none_or(|hash| hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
        && grants.command_groups.len() <= COMMAND_GROUPS.len()
        && grants.command_groups.iter().all(|group| COMMAND_GROUPS.contains(&group.as_str()))
        && grants.fuel.is_none_or(|fuel| (1..=MAX_APPROVED_FUEL).contains(&fuel))
        && grants.memory_mib.is_none_or(|memory| (1..=MAX_MEMORY_MIB).contains(&memory))
        // Folders chosen at install, one per grant, and the ones the user picked later for a
        // `picked` grant, each folder once (`validate_grants` tells them apart by the manifest).
        && grants.folders.len() <= 32 + 2 * MAX_PICKED_FOLDERS
        && grants.folders.iter().all(|grant| {
            let path = grant.folder.to_string_lossy();
            valid_id(&grant.name)
                && names.insert((&grant.name, &grant.folder))
                && (grant.read || grant.write)
                && grant.folder.is_absolute()
                && path.len() <= 4096
                && !path.chars().any(char::is_control)
        })
}

fn read_record(path: &Path) -> Option<Grants> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_RECORD_BYTES {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    if bytes.len() as u64 > MAX_RECORD_BYTES {
        return None;
    }
    serde_json::from_slice::<Grants>(&bytes).ok().filter(valid_record)
}

/// Whether the active file scope lets this session reach the store of `plugins`. A store outside
/// the scope is neither read nor written, as any other file there.
fn in_scope(plugins: &Path) -> bool {
    solidcraft_io::vfs::check(&folder(plugins).to_string_lossy()).is_ok()
}

/// Every readable record in the store of `plugins`, by plugin id. An absent store, or one outside
/// the active file scope, is empty.
pub fn read_all(plugins: &Path) -> BTreeMap<String, Grants> {
    let mut records = BTreeMap::new();
    if !in_scope(plugins) {
        return records;
    }
    let Ok(entries) = std::fs::read_dir(folder(plugins)) else { return records };
    for entry in entries.flatten().take(MAX_ENTRIES) {
        if records.len() >= MAX_RECORDS {
            break;
        }
        let name = entry.file_name();
        let Some(id) = name.to_str().and_then(|n| n.strip_suffix(".json")).filter(|id| valid_id(id)) else { continue };
        if let Some(grants) = read_record(&entry.path()) {
            records.insert(id.to_string(), grants);
        }
    }
    records
}

/// Write the record of plugin `id`, replacing the one stored.
pub fn write(plugins: &Path, id: &str, grants: &Grants) -> Result<(), String> {
    if !valid_id(id) || !valid_record(grants) {
        return Err(format!("`{id}`: not a plugin approval that can be stored"));
    }
    let dir = folder(plugins);
    // The scope must allow the record before its folder is created.
    solidcraft_io::vfs::check_new(&dir.join(format!("{id}.json")).to_string_lossy()).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut grants = grants.clone();
    grants.folders.sort_by(|a, b| a.name.cmp(&b.name));
    let bytes = serde_json::to_vec_pretty(&grants).map_err(|e| e.to_string())?;
    // A record too large to read back would lose the plugin's approvals at the next start.
    if bytes.len() as u64 > MAX_RECORD_BYTES {
        return Err(format!("`{id}`: its approvals are too large to keep"));
    }
    let stamp = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or_default();
    let staging = dir.join(format!(".{id}-{}-{stamp}.tmp", std::process::id()));
    let written = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&staging)
        .and_then(|mut f| std::io::Write::write_all(&mut f, &bytes).and_then(|()| f.sync_all()))
        .and_then(|()| std::fs::rename(&staging, dir.join(format!("{id}.json"))));
    if let Err(e) = written {
        let _ = std::fs::remove_file(&staging);
        return Err(e.to_string());
    }
    Ok(())
}

/// Delete the record of plugin `id`; an absent record is already deleted.
pub fn delete(plugins: &Path, id: &str) -> Result<(), String> {
    if !valid_id(id) {
        return Err(format!("`{id}` is not a plugin id"));
    }
    let record = folder(plugins).join(format!("{id}.json"));
    solidcraft_io::vfs::check_new(&record.to_string_lossy()).map_err(|e| e.to_string())?;
    match std::fs::remove_file(record) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
        _ => Ok(()),
    }
}

/// When the store's folder last changed: a record written, replaced or deleted. A change in the
/// last two seconds gives no stamp, so the store is read again: a file system's clock may be too
/// coarse to tell that change from one made right after it.
fn stamp(plugins: &Path) -> Option<SystemTime> {
    let changed = std::fs::metadata(folder(plugins)).and_then(|m| m.modified()).ok()?;
    SystemTime::now().duration_since(changed).is_ok_and(|age| age > std::time::Duration::from_secs(2)).then_some(changed)
}

/// What one host last read from or wrote to the store, so it can tell another program's changes
/// from its own. The host keeps its grants in a map of its own (`working`) and syncs it here.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Synced {
    grants: BTreeMap<String, Grants>,
    stamp: Option<SystemTime>,
}

impl Synced {
    /// Start from the store when it exists: it is then the whole truth, and `working` becomes
    /// what it holds. With no store yet, `working` (grants approved before there was one) is
    /// written to it.
    pub fn adopt(&mut self, plugins: &Path, working: &mut BTreeMap<String, Grants>) -> Result<(), String> {
        if folder(plugins).is_dir() {
            self.stamp = stamp(plugins);
            self.grants = read_all(plugins);
            working.clone_from(&self.grants);
            Ok(())
        } else {
            *self = Self::default();
            self.persist(plugins, working)
        }
    }

    /// Take in what another program changed since the last look: each record that differs from
    /// what was last seen replaces (or, deleted, removes) that plugin's grants in `working`.
    pub fn refresh(&mut self, plugins: &Path, working: &mut BTreeMap<String, Grants>) {
        // A store the file scope refuses reads as empty; that is no change by another program.
        if !in_scope(plugins) {
            return;
        }
        let now = stamp(plugins);
        if now.is_some() && now == self.stamp {
            return;
        }
        // Read after taking the stamp: a change made while reading leaves a newer one.
        let stored = read_all(plugins);
        let ids: BTreeSet<&String> = stored.keys().chain(self.grants.keys()).collect();
        for id in ids {
            if stored.get(id) != self.grants.get(id) {
                match stored.get(id) {
                    Some(grants) => {
                        working.insert(id.clone(), grants.clone());
                    }
                    None => {
                        working.remove(id);
                    }
                }
            }
        }
        self.grants = stored;
        self.stamp = now;
    }

    /// Delete the record of plugin `id`, including one this host never read (a store outside its
    /// file scope, or written since the last look).
    pub fn forget(&mut self, plugins: &Path, id: &str) -> Result<(), String> {
        delete(plugins, id)?;
        self.grants.remove(id);
        self.stamp = None;
        Ok(())
    }

    /// Write what changed in `working` since the last look: a changed record, or the deletion
    /// of one that is gone. The next [`Self::refresh`] reads the store again.
    pub fn persist(&mut self, plugins: &Path, working: &BTreeMap<String, Grants>) -> Result<(), String> {
        let ids: Vec<String> = working.keys().chain(self.grants.keys()).cloned().collect::<BTreeSet<_>>().into_iter().collect();
        let mut failed = None;
        for id in ids {
            let (now, seen) = (working.get(&id), self.grants.get(&id));
            if now == seen {
                continue;
            }
            let done = match now {
                Some(grants) => write(plugins, &id, grants),
                None => delete(plugins, &id),
            };
            match done {
                Ok(()) => {
                    match now {
                        Some(grants) => self.grants.insert(id, grants.clone()),
                        None => self.grants.remove(&id),
                    };
                    self.stamp = None;
                }
                Err(e) => {
                    failed.get_or_insert(format!("{id}: {e}"));
                }
            }
        }
        failed.map_or(Ok(()), Err)
    }
}
