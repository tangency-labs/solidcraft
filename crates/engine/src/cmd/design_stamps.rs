//! What the files in a design's work folder hold, remembered so they need not be read to be
//! compared (experimental, #6892). Opening a design used to read every unpacked internal
//! component to see whether it still holds what the design's file holds, and a save read each one
//! again to store it. With two thousand parts that is two thousand reads, and the first read of a
//! file the system has not seen (a design copied, unzipped or synced with its work folder) is slow.
//!
//! The work folder keeps a small record, [`FILE`]: for each file its size, its modified time and
//! the digest of its bytes when last read. A file whose size and time are as recorded holds the
//! recorded bytes, the rule [`super::component_link`] already uses for linked files. So:
//!
//! - a file of another size than the stored part differs, with no read;
//! - a file with the recorded size and time is compared by digest, with no read;
//! - anything else is read and compared byte for byte, as before, and recorded.
//!
//! The record is only ever a shortcut. One that is missing, damaged or out of date costs the
//! reads it would have saved, never a wrong answer. A record made within [`SETTLE_NS`] of the
//! file's own modified time is not trusted: on a file system that keeps time to the second or
//! two (FAT, some network shares), a file written twice in that window can keep its size and
//! time and hold other bytes. Such a file is read once more on a later open and recorded then.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use solidcraft_doc::DesignLink;

/// The record's file name in the work folder. No internal component can have it: a member's
/// name never starts with a dot ([`solidcraft_doc::internal::valid_name`]).
pub(crate) const FILE: &str = ".~stamps.json";
/// The largest record read: far more than [`solidcraft_doc::internal::MAX_FILES`] entries need.
const MAX_BYTES: u64 = 4 << 20;
/// A record is trusted only when it was made at least this long after the file was last modified.
const SETTLE_NS: u128 = 3_000_000_000;

/// One file as last read.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Entry {
    len: u64,
    /// Nanoseconds since the Unix epoch, as text: JSON numbers do not hold it exactly.
    modified_ns: String,
    recorded_ns: String,
    sha256: String,
}

/// The record of one work folder, read when asked for and written back when it changed.
#[derive(Debug)]
pub(crate) struct Stamps {
    folder: PathBuf,
    /// By file name in lowercase: names are told apart without regard to letter case.
    files: BTreeMap<String, Entry>,
    changed: bool,
}

fn now_ns() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos())
}

/// The digest [`Stamps`] compares by.
pub(crate) fn digest(bytes: &[u8]) -> String {
    DesignLink::file_digest(bytes)
}

impl Stamps {
    /// The record of the work folder `folder`: empty when it has none that reads.
    pub fn of(folder: &Path) -> Stamps {
        let files = (|| {
            let file = folder.join(FILE);
            let meta = std::fs::metadata(&file).ok()?;
            if !meta.is_file() || meta.len() > MAX_BYTES {
                return None;
            }
            serde_json::from_slice::<BTreeMap<String, Entry>>(&std::fs::read(&file).ok()?).ok()
        })()
        .unwrap_or_default();
        Stamps { folder: folder.to_path_buf(), files, changed: false }
    }

    /// The digest recorded for the file `name`, when its size and time are still as recorded and
    /// the record can be trusted.
    fn recorded(&self, name: &str, stamp: &solidcraft_io::design_lock::FileStamp) -> Option<&str> {
        let entry = self.files.get(&name.to_lowercase())?;
        let modified = stamp.modified_ns?;
        let settled = entry.recorded_ns.parse::<u128>().is_ok_and(|recorded| recorded >= modified.saturating_add(SETTLE_NS));
        (entry.len == stamp.len && entry.modified_ns == modified.to_string() && settled).then_some(entry.sha256.as_str())
    }

    /// Remember that the file `name`, as it is on disk now, holds bytes with the digest `sha256`.
    pub fn record(&mut self, name: &str, sha256: &str) {
        let Some(stamp) = solidcraft_io::design_lock::stamp(&self.folder.join(name).to_string_lossy()) else { return };
        let Some(modified) = stamp.modified_ns else { return };
        // Already recorded so, and trusted: nothing to write.
        if self.recorded(name, &stamp) == Some(sha256) {
            return;
        }
        let entry = Entry { len: stamp.len, modified_ns: modified.to_string(), recorded_ns: now_ns().to_string(), sha256: sha256.to_string() };
        self.files.insert(name.to_lowercase(), entry);
        self.changed = true;
    }

    /// Whether the file `name` holds exactly `stored`, whose digest `stored_sha256` gives when it
    /// is asked for. `None`: there is no such file, or it can't be read. The file is read only
    /// when its size matches and the record does not answer.
    pub fn holds(&mut self, name: &str, stored: &[u8], stored_sha256: impl FnOnce() -> String) -> Option<bool> {
        let file = self.folder.join(name);
        let stamp = solidcraft_io::design_lock::stamp(&file.to_string_lossy())?;
        if stamp.len != stored.len() as u64 {
            return Some(false);
        }
        if let Some(known) = self.recorded(name, &stamp) {
            return Some(known == stored_sha256());
        }
        let bytes = std::fs::read(&file).ok()?;
        #[cfg(test)]
        READS.with(|reads| reads.set(reads.get() + 1));
        self.record(name, &digest(&bytes));
        Some(bytes == stored)
    }

    /// Write the record back when it changed. A folder that can't be written keeps none, which
    /// only costs the reads.
    pub fn save(&mut self) {
        if !self.changed || cfg!(target_arch = "wasm32") {
            return;
        }
        self.changed = false;
        if !self.folder.is_dir() {
            return;
        }
        if let Ok(text) = serde_json::to_vec(&self.files) {
            let _ = solidcraft_io::write_atomic(&self.folder.join(FILE), &text, false);
        }
    }

    /// Remove the record's file: the folder is being emptied.
    pub fn remove(folder: &Path) {
        let _ = std::fs::remove_file(folder.join(FILE));
    }
}

#[cfg(test)]
thread_local! {
    /// How many work files this thread's tests have read to compare them.
    pub(crate) static READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// For tests: make every record in `folder` old enough to be trusted, as a later run of the app
/// finds it.
#[cfg(test)]
#[allow(clippy::unwrap_used)]
pub(crate) fn settle_all(folder: &Path) {
    let mut stamps = Stamps::of(folder);
    for entry in stamps.files.values_mut() {
        entry.recorded_ns = (entry.modified_ns.parse::<u128>().unwrap() + SETTLE_NS).to_string();
    }
    stamps.changed = true;
    stamps.save();
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::unreachable)]
mod tests {
    use super::*;

    fn folder(case: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("solidcraft-stamps-{case}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Make the record of `name` old enough to be trusted, as a later run of the app finds it.
    fn settle(stamps: &mut Stamps, name: &str) {
        let entry = stamps.files.get_mut(name).unwrap();
        entry.recorded_ns = (entry.modified_ns.parse::<u128>().unwrap() + SETTLE_NS).to_string();
    }

    #[test]
    fn a_work_file_is_compared_without_reading_it_once_it_is_recorded() {
        let dir = folder("compare");
        std::fs::write(dir.join("bracket.scprt"), b"stored bytes").unwrap();
        let mut stamps = Stamps::of(&dir);
        // No record yet: read, compared, recorded. No file: no answer.
        assert_eq!(stamps.holds("bracket.scprt", b"stored bytes", || unreachable!("read, not asked")), Some(true));
        assert_eq!(stamps.holds("missing.scprt", b"x", || unreachable!()), None);
        // Another size differs without a read or a digest.
        assert_eq!(stamps.holds("bracket.scprt", b"longer stored bytes", || unreachable!("the size says it")), Some(false));
        // A record made right after the file was written is not trusted yet: it is read again.
        assert_eq!(stamps.holds("bracket.scprt", b"stored bytes", || unreachable!("still read")), Some(true));
        stamps.save();
        assert!(dir.join(FILE).is_file());

        // Later: the record answers by digest, in either letter case, and the file is not read.
        let mut later = Stamps::of(&dir);
        settle(&mut later, "bracket.scprt");
        let mut asked = false;
        assert_eq!(
            later.holds("Bracket.scprt", b"stored bytes", || {
                asked = true;
                digest(b"stored bytes")
            }),
            Some(true)
        );
        assert!(asked, "answered from the record");
        assert_eq!(later.holds("bracket.scprt", b"other  bytes", || digest(b"other  bytes")), Some(false), "same size, other bytes");

        // The file changes: its time moves, the record no longer answers, and it is read.
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(dir.join("bracket.scprt"), b"edited bytes").unwrap();
        assert_eq!(later.holds("bracket.scprt", b"stored bytes", || unreachable!("read after a change")), Some(false));
        assert_eq!(later.holds("bracket.scprt", b"edited bytes", || unreachable!("fresh record, read")), Some(true));

        // A damaged record is no record, and the next save writes a good one over it.
        std::fs::write(dir.join(FILE), b"not json").unwrap();
        assert!(Stamps::of(&dir).files.is_empty());
        later.save();
        assert!(Stamps::of(&dir).files.contains_key("bracket.scprt"));
        Stamps::remove(&dir);
        assert!(!dir.join(FILE).exists());
        let _ = std::fs::remove_dir_all(dir);
    }
}
