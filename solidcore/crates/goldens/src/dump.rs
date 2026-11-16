//! `--dump-smb <dir>`: the bodies a case measures, written as `.smb` files so a test can load
//! the bodies the goldens measured (as the kernel rig's `KERNEL_CASES_SMB`); and [`capture`],
//! the same bodies kept in memory as exact copies, for a test that runs the cases itself.
//!
//! [`each_rust_output`](crate::runner::each_rust_output) turns dumping on around each case's
//! Rust side when [`RunOptions::dump_smb`](crate::runner::RunOptions::dump_smb) is set; a
//! runner hands [`body`] each body it measures, just before measuring it. Off (the
//! default), [`body`] does nothing, so a run produces exactly what it did before.
//!
//! The files are `<dir>/<unit>/<case>.<label>.<n>.smb`, `n` counting the case's bodies of
//! that label from 0 in the order the runner hands them over; the case name's characters
//! other than ASCII letters, digits, `-`, `_` and `.` become `_`, and a name changed that way
//! gets its hash appended ([`case_stem`]) so no two cases share a file. A failed write is printed
//! and does not change the case's output.
//!
//! An `.smb` file is not always the measured body exactly: a body whose UV trims the measure
//! makes can measure differently once read back, and the reader refuses some degenerate
//! curves the writer writes. [`capture`] keeps [`Brep::clone_standing_for_self`] copies,
//! which are exact.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::Kernel;
use solidcore::topology::api_files::write_brep_to_file;
use solidcore::topology::brep_data_write::FromBrepEnv;
use solidcore::topology::loopuse::notify_ref;
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::{Brep, EdgeId, EdgeuseId, FaceId};

/// One body [`capture`] kept: its label, its number among the case's bodies of that label,
/// and the copy (or why it could not be copied).
#[derive(Debug)]
pub struct Captured {
    /// The runner's label (`mass`, `shell_mass`, `volume`).
    pub label: String,
    /// The body's number among the case's bodies of `label`, from 0.
    pub n: usize,
    /// The exact copy.
    pub brep: Result<Brep, String>,
}

/// Where the current case's bodies go and how many of each label it has handed over.
#[derive(Default)]
struct Target {
    /// `<dir>/<unit>` and the case's file stem, when writing files.
    files: Option<(PathBuf, String)>,
    /// The copies, when capturing.
    captured: Option<Vec<Captured>>,
    written: BTreeMap<String, usize>,
}

thread_local! {
    static TARGET: RefCell<Option<Target>> = const { RefCell::new(None) };
}

/// `name` with every character but ASCII letters, digits, `-`, `_` and `.` as `_`.
pub fn file_stem(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// A case's file stem: [`file_stem`] of its name, and where that replaced a character, `-`
/// and the 64-bit FNV-1a hash of the name's bytes in hex, so two names that differ only in
/// replaced characters (`c/1`, `c 1`) get different files. A name [`file_stem`] keeps as it
/// is keeps its plain stem.
pub fn case_stem(case: &str) -> String {
    let stem = file_stem(case);
    if stem == case {
        return stem;
    }
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in case.bytes() {
        h ^= u64::from(byte);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{stem}-{h:016x}")
}

/// The file of body `n` of `label` of `case` of `unit` under `dir`.
pub fn path(dir: &Path, unit: &str, case: &str, label: &str, n: usize) -> PathBuf {
    dir.join(file_stem(unit))
        .join(format!("{}.{label}.{n}.smb", case_stem(case)))
}

/// Starts dumping case `case` of `unit` under `dir` on this thread.
pub fn start(dir: &Path, unit: &str, case: &str) {
    TARGET.with(|t| {
        *t.borrow_mut() = Some(Target {
            files: Some((dir.join(file_stem(unit)), case_stem(case))),
            ..Target::default()
        });
    });
}

/// Starts keeping copies of the bodies handed to [`body`] on this thread (no files); [`take`]
/// returns them.
pub fn capture() {
    TARGET.with(|t| {
        *t.borrow_mut() = Some(Target {
            captured: Some(Vec::new()),
            ..Target::default()
        });
    });
}

/// Stops capturing and returns the copies kept since [`capture`].
pub fn take() -> Vec<Captured> {
    TARGET.with(|t| {
        t.borrow_mut()
            .take()
            .and_then(|t| t.captured)
            .unwrap_or_default()
    })
}

/// Stops dumping on this thread.
pub fn stop() {
    TARGET.with(|t| *t.borrow_mut() = None);
}

/// Hands over `b`, the next body of `label`: written as an ASCII `.smb` (as the rig) when
/// dumping, copied when capturing, ignored otherwise.
pub fn body(k: &Kernel, label: &str, b: &Brep) {
    TARGET.with(|t| {
        let mut t = t.borrow_mut();
        let Some(target) = t.as_mut() else {
            return;
        };
        let slot = target.written.entry(label.to_string()).or_insert(0);
        let n = *slot;
        *slot += 1;
        if let Some((dir, case)) = &target.files {
            let file = dir.join(format!("{case}.{label}.{n}.smb"));
            if let Err(e) = write(k, b, &file) {
                println!("--dump-smb: {}: {e}", file.display());
            }
        }
        if let Some(kept) = &mut target.captured {
            kept.push(Captured {
                label: label.to_string(),
                n,
                brep: b.clone_standing_for_self().map_err(|e| e.to_string()),
            });
        }
    });
}

fn write(k: &Kernel, b: &Brep, file: &Path) -> Result<(), String> {
    let bytes = smb_bytes(k, b)?;
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(file, bytes).map_err(|e| e.to_string())
}

/// `b` as the contents of an ASCII `.smb` file, as [`body`] writes it.
///
/// # Errors
///
/// The writer's, as text.
pub fn smb_bytes(k: &Kernel, b: &Brep) -> Result<Vec<u8>, String> {
    let store = AttributeStore::default();
    let records = NotifyRecords::new();
    let env = FromBrepEnv {
        attributes: &store,
        key_of: &notify_ref,
        records: &records,
        curve_key: &|_: EdgeId| ObjectKey(0),
        uv_curve_key: &|_: EdgeuseId| ObjectKey(0),
        surface_key: &|_: FaceId| ObjectKey(0),
    };
    write_brep_to_file(k, Some(b), &env, true).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn case_names_differing_in_replaced_characters_get_different_files() {
        assert_eq!(case_stem("c_1"), "c_1");
        let (a, b) = (case_stem("c/1"), case_stem("c 1"));
        assert!(a.starts_with("c_1-") && b.starts_with("c_1-"), "{a} {b}");
        assert_ne!(a, b);
        assert_ne!(a, case_stem("c_1"));
        assert_eq!(a, case_stem("c/1"), "stable");
        let dir = Path::new("d");
        assert_ne!(
            path(dir, "u", "c/1", "mass", 0),
            path(dir, "u", "c_1", "mass", 0)
        );
    }
}
