//! Small offline render-resource checks, run by the existing attribution/CI gate.
//! Numerical authoring reproduction remains a separate, explicitly invoked tool.
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::Path;

fn object_file(root: &Path, relative: &str) -> Result<Value, String> {
    let path = root.join(relative);
    let metadata = std::fs::metadata(&path).map_err(|e| format!("{relative}: {e}"))?;
    if metadata.len() > 4 * 1024 * 1024 {
        return Err(format!("render manifest exceeds 4 MiB: {relative}"));
    }
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    serde_json::from_slice(&bytes).map_err(|e| format!("{relative}: {e}"))
}

fn verify(root: &Path, relative: &str, expected: &str) -> Result<(), String> {
    if Path::new(relative).is_absolute() || expected.len() != 64 || !expected.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()) {
        return Err(format!("invalid render source receipt: {relative}"));
    }
    let boundary = root.join("assets/render").canonicalize().map_err(|e| e.to_string())?;
    let path = root.join(relative).canonicalize().map_err(|e| format!("{relative}: {e}"))?;
    if !path.starts_with(boundary) || std::fs::metadata(&path).map_err(|e| e.to_string())?.len() > 16 * 1024 * 1024 {
        return Err(format!("render source outside resource boundary or over 16 MiB: {relative}"));
    }
    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
    if format!("{:x}", Sha256::digest(bytes)) != expected {
        return Err(format!("render resource SHA-256 mismatch: {relative}"));
    }
    Ok(())
}

pub fn run(root: &Path, files: &[String], attribution: &str) -> Result<(), String> {
    for relative in attribution.lines().filter(|line| line.starts_with('|')).filter_map(|line| line.split('|').nth(1)).map(str::trim) {
        if relative.starts_with("assets/render/") && !files.iter().any(|file| file == relative) && !root.join(relative).is_file() {
            return Err(format!("render attribution names an absent resource: {relative}"));
        }
    }
    if !root.join("assets/render/starter").is_dir() {
        return Ok(());
    }
    let surfaces = object_file(root, "assets/render/starter/materials/manifest.json")?;
    let items = surfaces.get("items").and_then(Value::as_array).ok_or("render surface manifest has no items")?;
    if items.is_empty() || items.len() > 1024 {
        return Err("render surface manifest requires 1..1024 items".into());
    }
    let mut checked = 0;
    for item in items {
        let maps = item.get("maps").and_then(Value::as_object).ok_or("render surface has no maps")?;
        for map in maps.values() {
            let path = map.get("path").and_then(Value::as_str).ok_or("render map has no path")?;
            let sha = map.get("sha256").and_then(Value::as_str).ok_or("render map has no SHA-256")?;
            verify(root, path, sha)?;
            checked += 1;
        }
    }
    let environments = object_file(root, "assets/render/starter/environments/manifest.json")?;
    let items = environments.get("items").and_then(Value::as_array).ok_or("render environment manifest has no items")?;
    if items.is_empty() || items.len() > 1024 {
        return Err("render environment manifest requires 1..1024 items".into());
    }
    for item in items {
        let path = item.get("path").and_then(Value::as_str).ok_or("render environment has no path")?;
        let sha = item.get("sha256").and_then(Value::as_str).ok_or("render environment has no SHA-256")?;
        verify(root, &format!("assets/render/starter/environments/{path}"), sha)?;
        checked += 1;
    }
    println!("render assets: {checked} encoded resources match their source receipts; no external packages or render jobs");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orphan_render_attribution_is_rejected_before_manifest_access() {
        let error = run(Path::new("."), &[], "| assets/render/absent.png | author | original | MIT |\n").unwrap_err();
        assert!(error.contains("absent resource"));
    }

    #[test]
    fn malformed_receipts_are_refused_before_file_access() {
        for digest in ["", "not-a-hash", &"A".repeat(64)] {
            assert!(verify(Path::new("."), "assets/render/absent.png", digest).unwrap_err().contains("invalid render source receipt"));
        }
    }
}
