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

/// The most reports kept waiting; a full queue refuses a new one.
pub const MAX_QUEUED: usize = 20;

/// Put `report` in the queue; its folder.
pub fn put(dir: &Path, report: &Outgoing) -> Result<PathBuf, String> {
    if list(dir).len() >= MAX_QUEUED {
        return Err(format!("{MAX_QUEUED} reports are already waiting to be sent."));
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
    for (i, file) in report.files.iter().enumerate() {
        std::fs::write(folder.join(format!("file-{i}")), &file.bytes).map_err(|e| e.to_string())?;
    }
    save(&folder, report)?;
    Ok(folder)
}

/// Record how far `report` got (filed, uploads opened or done).
pub fn save(folder: &Path, report: &Outgoing) -> Result<(), String> {
    let json = serde_json::to_vec_pretty(report).map_err(|e| e.to_string())?;
    let tmp = folder.join("report.json.tmp");
    std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, folder.join("report.json")).map_err(|e| e.to_string())
}

/// The waiting reports' folders, oldest first (not those the server refused).
pub fn list(dir: &Path) -> Vec<PathBuf> {
    let mut all: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("report.json").is_file() && !p.join("refused").exists())
        .collect();
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

/// The server refused it: it stops waiting, and stays on disk for its user.
pub fn refused(folder: &Path) {
    let _ = std::fs::write(folder.join("refused"), b"");
}
