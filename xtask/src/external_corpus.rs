//! `cargo xtask external-corpus [--bless "why"] [--only NAME] [--cli PATH]`: nightly import
//! checks on real-world STEP files (#1397).
//!
//! `tests/external-corpus.toml` lists each source archive (URL, SHA-256, licence) and every
//! file inside it (path, SHA-256, expected results). The run downloads each archive once into
//! the gitignored `corpus/external/`, verifies every hash, imports each file headless with
//! `solidcraft-cli` (`file.insert_step` then `inspect.measure`), and checks the body count,
//! total volume and area within tolerances and the time and memory budgets. Import results
//! should improve over time, so expectations are re-recorded with a reason:
//! `--bless "why"` rewrites the expected values in the manifest. A file whose hash no longer
//! matches upstream is reported as "source changed" and never counted as our failure.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime};

use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const MANIFEST: &str = "tests/external-corpus.toml";
const CACHE_DIR: &str = "corpus/external";

/// Defaults for entries that do not set their own.
const DEFAULT_VOLUME_REL_TOL: f64 = 0.001; // the issue: volume within ~0.1 %
const DEFAULT_AREA_REL_TOL: f64 = 0.005; // area drifts more than volume as healing improves
const DEFAULT_SECONDS_MAX: u64 = 300;
const DEFAULT_MEMORY_MB_MAX: u64 = 8192;

#[derive(Deserialize)]
struct Source {
    name: String,
    url: String,
    /// Only "zip" is supported: an archive is unpacked into the cache under the source name.
    archive: String,
    archive_sha256: String,
    licence: String,
    #[allow(dead_code)] // shown for the reader of the manifest; the runner does not need it
    note: Option<String>,
}

#[derive(Deserialize)]
struct Entry {
    name: String,
    source: String,
    path: String,
    sha256: String,
    /// Expected results; absent until `--bless` records them.
    bodies: Option<i64>,
    volume_mm3: Option<f64>,
    area_mm2: Option<f64>,
    volume_rel_tol: Option<f64>,
    area_rel_tol: Option<f64>,
    seconds_max: Option<u64>,
    memory_mb_max: Option<u64>,
    #[allow(dead_code)] // written by --bless so a reviewer can see why the numbers moved
    blessed: Option<String>,
}

#[derive(Deserialize)]
struct Manifest {
    source: Vec<Source>,
    file: Vec<Entry>,
}

/// What one file's import produced, for the report and for --bless.
struct Measured {
    bodies: i64,
    volume_mm3: f64,
    area_mm2: f64,
    seconds: f64,
    memory_mb: Option<f64>,
}

pub fn run(root: &Path, rest: &[&str]) -> Result<(), String> {
    let arg = |name: &str| rest.iter().position(|a| *a == name).and_then(|i| rest.get(i + 1)).copied();
    let bless_reason = match rest.iter().position(|a| *a == "--bless") {
        Some(_) => Some(arg("--bless").ok_or("--bless needs a reason: --bless \"why the numbers moved\"")?.to_string()),
        None => None,
    };
    let only = arg("--only");
    let cli: Vec<String> = match arg("--cli") {
        Some(path) => vec![path.to_string()],
        None => {
            let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
            vec![cargo, "run".into(), "-q".into(), "--profile".into(), "ci".into(), "-p".into(), "solidcraft-cli".into(), "--".into()]
        }
    };

    let manifest_path = root.join(MANIFEST);
    let text = std::fs::read_to_string(&manifest_path).map_err(|e| format!("{}: {e}", manifest_path.display()))?;
    let manifest: Manifest = toml::from_str(&text).map_err(|e| format!("{}: {e}", manifest_path.display()))?;
    let sources: BTreeMap<&str, &Source> = manifest.source.iter().map(|s| (s.name.as_str(), s)).collect();

    let cache = root.join(CACHE_DIR);
    std::fs::create_dir_all(&cache).map_err(|e| format!("{}: {e}", cache.display()))?;

    let mut pass = 0usize;
    let mut recorded = 0usize;
    let mut source_changed = Vec::new();
    let mut failed: Vec<String> = Vec::new();
    let mut measured: BTreeMap<String, Measured> = BTreeMap::new();

    for entry in &manifest.file {
        if only.is_some_and(|o| o != entry.name) {
            continue;
        }
        let source = sources.get(&entry.source.as_str()).ok_or_else(|| format!("{}: unknown source '{}'", entry.name, entry.source))?;
        let unpacked = ensure_source(&cache, source)?;
        let file = unpacked.join(&entry.path);
        let actual = sha256_file(&file)?;
        if !actual.eq_ignore_ascii_case(&entry.sha256) {
            source_changed.push(format!("{} (upstream hash is now {actual})", entry.name));
            continue;
        }
        match import_and_measure(&cli, &cache, entry, &file) {
            Ok(m) => {
                measured.insert(entry.name.clone(), m);
                let m = &measured[&entry.name];
                if entry.volume_mm3.is_none() {
                    recorded += 1;
                    println!(
                        "  record {:<27} {:>14.3} mm3, {:>14.3} mm2, {} bodies, {:.1} s (no expected values yet)",
                        entry.name, m.volume_mm3, m.area_mm2, m.bodies, m.seconds
                    );
                } else {
                    let problems = check(entry, m);
                    if problems.is_empty() {
                        pass += 1;
                        println!(
                            "  pass   {:<27} {:>14.3} mm3, {:>14.3} mm2, {} bodies, {:.1} s{}",
                            entry.name,
                            m.volume_mm3,
                            m.area_mm2,
                            m.bodies,
                            m.seconds,
                            m.memory_mb.map_or(String::new(), |mb| format!(", {mb:.0} MB"))
                        );
                    } else {
                        for p in problems {
                            failed.push(format!("{}: {p}", entry.name));
                        }
                    }
                }
            }
            Err(e) => failed.push(format!("{}: {e}", entry.name)),
        }
    }

    if let Some(reason) = &bless_reason {
        let stamp = bless_stamp(reason);
        let rewritten = rewrite_expected(&text, &measured, &stamp);
        std::fs::write(&manifest_path, rewritten).map_err(|e| format!("{}: {e}", manifest_path.display()))?;
        println!("external-corpus: re-recorded {} entries ({})", measured.len(), reason);
    }

    println!("external-corpus: {pass} pass, {recorded} recorded, {} failed, {} source changed", failed.len(), source_changed.len());
    for f in &failed {
        println!("  FAIL {f}");
    }
    for s in &source_changed {
        println!("  SOURCE CHANGED {s} — re-download, re-hash and re-record the entry; this is not our failure");
    }
    if !failed.is_empty() {
        return Err(format!("external-corpus: {} file(s) failed", failed.len()));
    }
    Ok(())
}

/// Download and unpack one source into the cache unless it is already there at the pinned
/// archive hash. Data only: nothing from the archive is ever executed.
fn ensure_source(cache: &Path, source: &Source) -> Result<PathBuf, String> {
    if source.archive != "zip" {
        return Err(format!("{}: archive '{}' is not supported (only \"zip\")", source.name, source.archive));
    }
    let dir = cache.join(&source.name);
    let archive = cache.join(format!("{}.download", source.name));
    let marker = dir.join(".fetched");
    if marker.is_file() && std::fs::read_to_string(&marker).unwrap_or_default().trim() == source.archive_sha256 {
        return Ok(dir);
    }
    std::fs::create_dir_all(cache).map_err(|e| format!("{}: {e}", cache.display()))?;
    println!("external-corpus: downloading {} ({})", source.url, source.licence);
    let status =
        Command::new("curl").args(["-fsSL", "--retry", "3", "-o"]).arg(&archive).arg(&source.url).status().map_err(|e| format!("curl: {e}"))?;
    if !status.success() {
        return Err(format!("{}: download failed with {status}", source.name));
    }
    let actual = sha256_file(&archive)?;
    if !actual.eq_ignore_ascii_case(&source.archive_sha256) {
        let _ = std::fs::remove_file(&archive);
        return Err(format!("{}: archive hash is {actual}, manifest says {}", source.name, source.archive_sha256));
    }
    // Unpack fresh: a stale tree must not survive a re-pin.
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    unpack_zip(&archive, &dir)?;
    let _ = std::fs::remove_file(&archive);
    std::fs::write(&marker, &source.archive_sha256).map_err(|e| format!("{}: {e}", marker.display()))?;
    Ok(dir)
}

/// Unpack a zip, keeping only regular files and refusing entries that would escape the cache
/// (absolute paths or `..` components).
fn unpack_zip(archive: &Path, dir: &Path) -> Result<(), String> {
    let f = std::fs::File::open(archive).map_err(|e| format!("{}: {e}", archive.display()))?;
    let mut zip = zip::ZipArchive::new(f).map_err(|e| format!("{}: {e}", archive.display()))?;
    for i in 0..zip.len() {
        let mut item = zip.by_index(i).map_err(|e| format!("entry {i}: {e}"))?;
        // zip 9 makes the raw name a Result (it may not decode) and `enclosed_name` is None
        // when the entry would escape the archive root.
        let raw = item.name().map(|n| n.to_string()).unwrap_or_else(|_| format!("entry {i}"));
        let Some(rel) = item.enclosed_name() else {
            return Err(format!("{}: entry {raw} escapes the archive root", archive.display()));
        };
        if !item.is_file() {
            continue;
        }
        let out = dir.join(&rel);
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        let mut dest = std::fs::File::create(&out).map_err(|e| format!("{}: {e}", out.display()))?;
        std::io::copy(&mut item, &mut dest).map_err(|e| format!("{}: {e}", out.display()))?;
    }
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut f = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf).map_err(|e| format!("{}: {e}", path.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Import the file and measure the result in one headless CLI run; returns what was measured.
/// Err means the import itself failed (a crash, a command error, unparsable output).
fn import_and_measure(cli: &[String], cache: &Path, entry: &Entry, file: &Path) -> Result<Measured, String> {
    let work = cache.join("_work");
    std::fs::create_dir_all(&work).map_err(|e| format!("{}: {e}", work.display()))?;
    let script = work.join(format!("{}.json", entry.name));
    let out = work.join(format!("{}.out.json", entry.name));
    let steps = json!([
        {"command": "file.insert_step", "params": {"path": file.display().to_string()}},
        {"command": "inspect.measure", "params": {}}
    ]);
    std::fs::write(&script, serde_json::to_string(&steps).map_err(|e| e.to_string())?).map_err(|e| format!("{}: {e}", script.display()))?;

    // GNU time, when present, reports the child's peak resident set; without it the memory
    // budget is not measured (the time budget still is).
    let gnu_time = gnu_time();
    let mem_file = work.join(format!("{}.mem", entry.name));
    let mut cmd = match &gnu_time {
        Some(t) => {
            let mut c = Command::new(t);
            c.args(["-f", "%M", "-o"]).arg(&mem_file);
            c
        }
        None => Command::new(&cli[0]),
    };
    if gnu_time.is_some() {
        cmd.args(&cli[1..]);
    }
    let started = Instant::now();
    cmd.args(["run"]).arg(&script).args(["--results", "--json-out"]).arg(&out).stdout(Stdio::null()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| format!("{}: failed to spawn: {e}", cli.join(" ")))?;
    let limit = Duration::from_secs(entry.seconds_max.unwrap_or(DEFAULT_SECONDS_MAX));
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        if started.elapsed() > limit {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("import took over {} s (budget) — a hang or a blow-up", limit.as_secs()));
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let seconds = started.elapsed().as_secs_f64();
    if !status.success() {
        let stderr = child.stderr.take().map(|mut e| {
            let mut t = String::new();
            let _ = e.read_to_string(&mut t);
            t.trim().chars().take(400).collect::<String>()
        });
        return Err(format!(
            "import failed ({status}){}",
            stderr.map(|s| if s.is_empty() { String::new() } else { format!(": {s}") }).unwrap_or_default()
        ));
    }
    let raw = std::fs::read_to_string(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    let results: Value = serde_json::from_str(&raw).map_err(|e| format!("{}: {e}", out.display()))?;
    let step = |i: usize| -> Result<&Value, String> { results.get(i).ok_or_else(|| format!("no result {i} in {}", out.display())) };
    let inserted = step(0)?;
    if let Some(err) = inserted.get("error") {
        return Err(format!("file.insert_step: {err}"));
    }
    let measured = step(1)?;
    if let Some(err) = measured.get("error") {
        return Err(format!("inspect.measure: {err}"));
    }
    let bodies = measured["body_count"].as_i64().ok_or("inspect.measure: no body_count")?;
    if bodies == 0 {
        return Err("import produced no bodies".into());
    }
    let volume_mm3 = measured["total"]["volume_mm3"].as_f64().ok_or("inspect.measure: no total volume")?;
    let area_mm2 = measured["total"]["area_mm2"].as_f64().ok_or("inspect.measure: no total area")?;
    // A solid measures with positive volume; a non-closed shell measures zero or negative.
    if volume_mm3 <= 0.0 {
        return Err(format!("not a closed solid: total volume {volume_mm3} mm3"));
    }
    let memory_mb = if gnu_time.is_some() {
        std::fs::read_to_string(&mem_file).ok().and_then(|t| t.trim().parse::<f64>().ok()).map(|kb| kb / 1024.0)
    } else {
        None
    };
    Ok(Measured { bodies, volume_mm3, area_mm2, seconds, memory_mb })
}

/// The checks against the manifest's expectations; empty means the file passed.
fn check(entry: &Entry, m: &Measured) -> Vec<String> {
    let mut problems = Vec::new();
    let Some(want_volume) = entry.volume_mm3 else {
        return problems; // no expected values yet: recorded, nothing to check
    };
    if let Some(want) = entry.bodies.filter(|want| m.bodies != *want) {
        problems.push(format!("body count {} is not the expected {want}", m.bodies));
    }
    let rel = (m.volume_mm3 - want_volume).abs() / want_volume.abs();
    if rel > entry.volume_rel_tol.unwrap_or(DEFAULT_VOLUME_REL_TOL) {
        problems.push(format!(
            "volume {:.3} mm3 is {:+.2} % off the expected {want_volume:.3} (tolerance {:.2} %)",
            m.volume_mm3,
            (m.volume_mm3 / want_volume - 1.0) * 100.0,
            entry.volume_rel_tol.unwrap_or(DEFAULT_VOLUME_REL_TOL) * 100.0
        ));
    }
    if let Some(want_area) = entry.area_mm2 {
        let rel = (m.area_mm2 - want_area).abs() / want_area.abs();
        if rel > entry.area_rel_tol.unwrap_or(DEFAULT_AREA_REL_TOL) {
            problems.push(format!(
                "area {:.3} mm2 is {:+.2} % off the expected {want_area:.3} (tolerance {:.2} %)",
                m.area_mm2,
                (m.area_mm2 / want_area - 1.0) * 100.0,
                entry.area_rel_tol.unwrap_or(DEFAULT_AREA_REL_TOL) * 100.0
            ));
        }
    }
    if m.seconds > entry.seconds_max.unwrap_or(DEFAULT_SECONDS_MAX) as f64 {
        problems.push(format!("import took {:.1} s, over the budget of {} s", m.seconds, entry.seconds_max.unwrap_or(DEFAULT_SECONDS_MAX)));
    }
    if let Some(mb) = m.memory_mb {
        let max = entry.memory_mb_max.unwrap_or(DEFAULT_MEMORY_MB_MAX);
        if mb > max as f64 {
            problems.push(format!("peak memory {mb:.0} MB is over the budget of {max} MB"));
        }
    }
    problems
}

/// GNU time's path if `/usr/bin/time` is GNU time (its `%M` is the peak RSS in kB); None
/// elsewhere, which skips the memory budget rather than guessing a platform's units.
fn gnu_time() -> Option<String> {
    let out = Command::new("/usr/bin/time").arg("--version").stderr(Stdio::null()).output().ok()?;
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    text.contains("GNU time").then(|| "/usr/bin/time".to_string())
}

/// `--bless "why"`: a stamp recorded with each re-recorded entry, so the manifest says when
/// and why the numbers last moved.
fn bless_stamp(reason: &str) -> String {
    let days = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs() / 86_400).unwrap_or(0);
    let (y, m, d) = civil_from_days(days as i64);
    format!("{y:04}-{m:02}-{d:02}: {reason}")
}

/// Days since the Unix epoch to a civil date (Howard Hinnant's algorithm).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Rewrite the expected-value lines of each blessed entry's `[[file]]` block in place, so
/// comments and hand-written keys stay as they are.
fn rewrite_expected(text: &str, measured: &BTreeMap<String, Measured>, stamp: &str) -> String {
    let mut lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
    let mut i = 0;
    while i < lines.len() {
        if lines[i].trim() != "[[file]]" {
            i += 1;
            continue;
        }
        let start = i;
        let mut end = start + 1;
        while end < lines.len() && !lines[end].trim_start().starts_with("[[") {
            end += 1;
        }
        let name = lines[start + 1..end]
            .iter()
            .find_map(|l| l.trim().strip_prefix("name = \""))
            .and_then(|r| r.strip_suffix('"'))
            .unwrap_or_default()
            .to_string();
        let Some(m) = measured.get(&name) else {
            i = end;
            continue;
        };
        let want: Vec<(&str, String)> = vec![
            ("bodies", m.bodies.to_string()),
            ("volume_mm3", format!("{:.3}", m.volume_mm3)),
            ("area_mm2", format!("{:.3}", m.area_mm2)),
            ("blessed", format!("\"{stamp}\"")),
        ];
        for (key, value) in want {
            let prefix = format!("{key} = ");
            match lines[start + 1..end].iter().position(|l| l.trim_start().starts_with(&prefix)) {
                Some(at) => lines[start + 1 + at] = format!("{key} = {value}"),
                None => {
                    // Insert after the last expected-value line we own, or after `sha256`.
                    let anchor = lines[start + 1..end]
                        .iter()
                        .rposition(|l| {
                            let t = l.trim_start();
                            t.starts_with("sha256 = ")
                                || t.starts_with("bodies = ")
                                || t.starts_with("volume_mm3 = ")
                                || t.starts_with("area_mm2 = ")
                                || t.starts_with("blessed = ")
                        })
                        .unwrap_or(0);
                    lines.insert(start + 1 + anchor + 1, format!("{key} = {value}"));
                    end += 1;
                }
            }
        }
        i = end;
    }
    let mut out = lines.join("\n");
    if text.ends_with('\n') {
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::civil_from_days;

    #[test]
    fn epoch_days_to_civil_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29)); // a leap day
        assert_eq!(civil_from_days(20_751), (2026, 10, 25)); // the day this corpus was first recorded
    }
}
