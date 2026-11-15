//! Reports that could not be sent (offline) wait in a folder and go when the network is back:
//! one folder per report, `report.json` (the ticket, what was uploaded already) and its files.
//! Nothing in it is a secret: the token stays in the keyring and is read when a report goes.

use std::path::{Path, PathBuf};

use super::payload::Outgoing;

/// The queue folder: `<data folder>/report-queue`; None where there is no disk (the browser).
pub fn default_dir() -> Option<PathBuf> {
    #[cfg(target_arch = "wasm32")]
    {
        None
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        solidcraft_engine::recovery::data_dir().map(|d| d.join("report-queue"))
    }
}

/// The most reports kept waiting, and the most bytes their files take; a full queue refuses a
/// new one.
pub const MAX_QUEUED: usize = 20;
pub const MAX_QUEUED_BYTES: u64 = 1 << 30;

/// The bytes the queue's files take on disk.
fn queued_bytes(dir: &Path) -> u64 {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .flat_map(|e| std::fs::read_dir(e.path()).into_iter().flatten().flatten())
        .filter_map(|f| f.metadata().ok().map(|m| m.len()))
        .sum()
}

/// Put `report` in the queue; its folder. A report that cannot be kept whole leaves nothing.
pub fn put(dir: &Path, report: &Outgoing) -> Result<PathBuf, String> {
    if list(dir).len() >= MAX_QUEUED {
        return Err(format!("{MAX_QUEUED} reports are already waiting to be sent."));
    }
    let size: u64 = report.files.iter().map(|f| f.bytes.len() as u64).sum();
    if queued_bytes(dir).saturating_add(size) > MAX_QUEUED_BYTES {
        return Err(format!("the reports waiting to be sent already take {} MB.", MAX_QUEUED_BYTES >> 20));
    }
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let mut n = 0u32;
    let folder = loop {
        let f = dir.join(format!("{stamp}-{n}"));
        if !f.exists() {
            break f;
        }
        n += 1;
    };
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let written = report
        .files
        .iter()
        .enumerate()
        .try_for_each(|(i, file)| std::fs::write(folder.join(format!("file-{i}")), &file.bytes).map_err(|e| e.to_string()))
        .and_then(|()| save(&folder, report));
    match written {
        Ok(()) => Ok(folder),
        Err(e) => {
            // Half a report is no report: its files go.
            remove(&folder);
            Err(e)
        }
    }
}

/// Record how far `report` got (filed, uploads opened or done).
pub fn save(folder: &Path, report: &Outgoing) -> Result<(), String> {
    let json = serde_json::to_vec_pretty(report).map_err(|e| e.to_string())?;
    let tmp = folder.join("report.json.tmp");
    std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, folder.join("report.json")).map_err(|e| e.to_string())
}

/// The waiting reports' folders, oldest first.
pub fn list(dir: &Path) -> Vec<PathBuf> {
    let mut all: Vec<PathBuf> =
        std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.join("report.json").is_file()).collect();
    all.sort();
    all
}

/// A waiting report with its files.
pub fn load(folder: &Path) -> Result<Outgoing, String> {
    let text = std::fs::read(folder.join("report.json")).map_err(|e| e.to_string())?;
    let mut report: Outgoing = serde_json::from_slice(&text).map_err(|e| e.to_string())?;
    for (i, file) in report.files.iter_mut().enumerate() {
        if !file.done {
            file.bytes = std::fs::read(folder.join(format!("file-{i}"))).map_err(|e| e.to_string())?;
        }
    }
    Ok(report)
}

/// Sent: forget it.
pub fn remove(folder: &Path) {
    let _ = std::fs::remove_dir_all(folder);
}
