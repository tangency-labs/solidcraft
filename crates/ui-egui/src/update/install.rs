//! One-click updates (docs/book/src/guide/updates.md): download the package that fits this
//! installation, verify it, and put it in place.
//!
//! Verification is never skipped. The package's SHA-256 must match the release's
//! `SHA256SUMS.txt`, fetched over HTTPS from the same release. When SolidCraft is built with an
//! update signing key ([`PUBLIC_KEY`]), `SHA256SUMS.txt.minisig` must also be a valid minisign
//! signature by that key whose trusted comment names the release, so a compromised download host
//! alone can't push a package. Anything that doesn't verify is deleted and nothing is replaced.
//!
//! What "put in place" means per installation:
//! - AppImage: the new AppImage is written next to the old one and renamed over it (atomic on
//!   one file system; the running copy keeps its old inode), then SolidCraft offers a restart.
//! - Windows installer: the `.msi` is started with `msiexec /i … /passive` as SolidCraft exits.
//! - Windows portable `.zip` / standalone `.exe`: the running `solidcraft.exe` is renamed to
//!   `solidcraft.exe.old` (Windows allows renaming a running executable), the new one takes its
//!   name, and SolidCraft offers a restart; the `.old` file is deleted at the next start.
//! - macOS: the `.dmg` is verified and opened; the user drags the app to Applications. SolidCraft
//!   is ad-hoc signed and not notarized, so replacing the bundle in place would install code the
//!   system can't vouch for without the user seeing it.
//! - Linux `.tar.gz`: the new archive is unpacked over the installed folder (the one that contains
//!   `bin/solidcraft`, or the program's own folder) and SolidCraft offers a restart. The previous
//!   folder is kept beside it as `<name>.old` until the next start. A folder that can't be renamed
//!   is left as it was.
//! - Flatpak, Snap, `.deb`, `.rpm`: never replaced; the window shows the command.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use sha2::{Digest, Sha256};

use super::{FetchError, InstallKind, Release};

/// The minisign public key releases are signed with (the base64 line of `minisign.pub`), or None
/// while the project has no update signing key: then the SHA-256 check over HTTPS is what
/// guards an update. See docs/releasing.md, "Update signing".
pub const PUBLIC_KEY: Option<&str> = None;

/// The checksum list every release carries, and its signature.
pub const SUMS: &str = "SHA256SUMS.txt";
pub const SUMS_SIG: &str = "SHA256SUMS.txt.minisig";
/// No package is bigger than this (they are about 30 MB).
pub const MAX_PACKAGE: u64 = 1 << 30;
/// Nor a single file inside the portable zip.
const MAX_UNZIPPED: u64 = 1 << 30;

/// The host's HTTPS download to a file: calls `progress(done, total)` as it goes and stops when
/// that returns false.
pub type HttpDownload = Arc<dyn Fn(&str, &Path, &dyn Fn(u64, Option<u64>) -> bool) -> Result<(), FetchError> + Send + Sync>;

/// How an installation is updated in place.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Method {
    /// Replace this AppImage file.
    AppImage(PathBuf),
    /// Run the downloaded `.msi`.
    Msi,
    /// Swap the running executable (at this path) for the portable zip's one.
    PortableZip(PathBuf),
    /// Swap the running executable (at this path) for the downloaded `.exe`.
    Exe(PathBuf),
    /// Open the downloaded disk image.
    Dmg,
    /// Replace this unpacked `.tar.gz` install (the folder that contains the program).
    Tarball(PathBuf),
}

/// The in-place update for an installation, if it has one. `exe` is the running executable.
pub fn method(kind: &InstallKind, exe: Option<&Path>) -> Option<Method> {
    match kind {
        InstallKind::AppImage(path) => Some(Method::AppImage(path.clone())),
        InstallKind::WindowsMsi => Some(Method::Msi),
        InstallKind::WindowsPortable => exe.map(|e| Method::PortableZip(e.to_path_buf())),
        InstallKind::WindowsExe => exe.map(|e| Method::Exe(e.to_path_buf())),
        InstallKind::MacApp(_) => Some(Method::Dmg),
        InstallKind::LinuxTarball => exe.and_then(installable_tarball_prefix).map(Method::Tarball),
        _ => None,
    }
}

/// The folder an unpacked Linux install lives in: the parent of `bin/` when the program is
/// `…/bin/solidcraft`, otherwise the program's own folder. `/` is not an install folder.
pub fn tarball_prefix(exe: &Path) -> Option<PathBuf> {
    let parent = exe.parent()?;
    let prefix = if parent.file_name().is_some_and(|name| name == "bin") { parent.parent()?.to_path_buf() } else { parent.to_path_buf() };
    prefix.parent().is_some().then_some(prefix)
}

/// A `.tar.gz` folder Update now may replace. A top-level directory (`/opt`, `/usr/local` is under
/// `/usr` and is not this kind) or the user's home would mean renaming far more than the install.
fn installable_tarball_prefix(exe: &Path) -> Option<PathBuf> {
    let prefix = tarball_prefix(exe)?;
    let parent = prefix.parent()?;
    if parent.parent().is_none() {
        return None;
    }
    let home = std::env::var_os("HOME").into_iter().chain(std::env::var_os("USERPROFILE"));
    if home.into_iter().any(|dir| prefix == Path::new(&dir)) {
        return None;
    }
    Some(prefix)
}

/// What to do once the package is verified and in place.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The new version replaced this one: start `program` after SolidCraft exits.
    Restart(PathBuf),
    /// Start this installer (program, arguments) after SolidCraft exits.
    Installer(PathBuf, Vec<String>),
    /// The disk image was opened for the user.
    Opened(PathBuf),
}

/// `SHA256SUMS.txt` lines (`<hex>  <name>` or `<hex> *<name>`): the lowercase hex for `name`.
pub fn expected_sha256(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (hash, file) = line.trim().split_once(char::is_whitespace)?;
        let file = file.trim_start().trim_start_matches('*');
        (file == name && hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit())).then(|| hash.to_ascii_lowercase())
    })
}

/// The SHA-256 of a file, in lowercase hex.
pub fn sha256_file(path: &Path) -> Result<String, String> {
    let mut f = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf).map_err(|e| format!("{}: {e}", path.display()))?;
        if n == 0 {
            break;
        }
        h.update(buf.get(..n).unwrap_or_default());
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// The command that runs an installer, waits for it to finish and then starts `exe` (the
/// installed program), so the app comes back after an update. Windows has no `wait && start` in
/// a plain process spawn, so PowerShell does the waiting; the installer's exit code is ignored on
/// purpose: when the user cancels it, the old version is still there to start.
pub fn relaunch_after(installer: &Path, args: &[String], exe: &Path) -> (PathBuf, Vec<String>) {
    // PowerShell single-quoted string: only `'` needs doubling.
    let q = |s: &str| format!("'{}'", s.replace('\'', "''"));
    // msiexec splits its command line itself, so an argument with spaces needs its own quotes.
    let list = args.iter().map(|a| if a.contains(' ') { q(&format!("\"{a}\"")) } else { q(a) }).collect::<Vec<_>>().join(",");
    let script = format!(
        "Start-Process -FilePath {} -ArgumentList {list} -Wait; Start-Process -FilePath {}",
        q(&installer.to_string_lossy()),
        q(&exe.to_string_lossy())
    );
    (
        PathBuf::from("powershell"),
        vec!["-NoProfile".into(), "-NonInteractive".into(), "-WindowStyle".into(), "Hidden".into(), "-Command".into(), script],
    )
}

/// `sums` is signed by `key` (minisign, prehashed) with a trusted comment naming `tag`.
pub fn verify_signature(sums: &[u8], signature: &str, key: &str, tag: &str) -> Result<(), String> {
    let key = minisign_verify::PublicKey::from_base64(key.trim()).map_err(|e| format!("update signing key: {e}"))?;
    let sig = minisign_verify::Signature::decode(signature).map_err(|e| format!("{SUMS_SIG}: {e}"))?;
    key.verify(sums, &sig, false).map_err(|e| format!("{SUMS} isn't signed by SolidCraft's release key: {e}"))?;
    // The comment is covered by the signature: an older release's signed list can't stand in.
    let comment = sig.trusted_comment();
    if !comment.split_whitespace().any(|w| w == tag) {
        return Err(format!("{SUMS_SIG} is for `{comment}`, not {tag}"));
    }
    Ok(())
}

/// Progress of a running update, shared with the UI.
#[derive(Default)]
pub struct Progress {
    pub done: AtomicU64,
    pub total: AtomicU64,
    pub cancel: AtomicBool,
}

/// What the update needs from the host.
pub struct Net {
    pub get: super::HttpGet,
    pub download: HttpDownload,
}

/// Download, verify and apply `release` for `method`. Runs on a background thread. `staging` is
/// a folder for downloads that aren't written next to their target (the `.msi`, the `.dmg`).
pub fn run(
    release: &Release,
    kind: &InstallKind,
    method: &Method,
    arch: &str,
    net: &Net,
    staging: &Path,
    progress: &Progress,
) -> Result<Outcome, String> {
    let asset = release.asset_for(kind, arch).ok_or_else(|| format!("this release has no package for a {} on {arch}", kind.label()))?;
    if asset.size > MAX_PACKAGE {
        return Err(format!("{} is too large ({} bytes)", asset.name, asset.size));
    }
    let sums_url = release.assets.iter().find(|a| a.name == SUMS).map(|a| a.url.clone()).ok_or_else(|| format!("this release has no {SUMS}"))?;
    let sums = (net.get)(&sums_url).map_err(|e| format!("{SUMS}: {e}"))?;
    if let Some(key) = PUBLIC_KEY {
        let sig_url = release
            .assets
            .iter()
            .find(|a| a.name == SUMS_SIG)
            .map(|a| a.url.clone())
            .ok_or_else(|| format!("this release isn't signed (no {SUMS_SIG})"))?;
        let sig = (net.get)(&sig_url).map_err(|e| format!("{SUMS_SIG}: {e}"))?;
        verify_signature(&sums, &String::from_utf8_lossy(&sig), key, &release.tag)?;
    }
    let sums = String::from_utf8_lossy(&sums);
    let expected = expected_sha256(&sums, &asset.name).ok_or_else(|| format!("{SUMS} doesn't list {}", asset.name))?;

    // Where the download goes: beside its target when it replaces a file (one file system, so
    // the final rename is atomic), else in the staging folder.
    let part = match method {
        Method::AppImage(target) => sibling(target, ".update")?,
        Method::Exe(exe) => sibling(exe, ".update")?,
        Method::Msi | Method::Dmg | Method::PortableZip(_) | Method::Tarball(_) => {
            std::fs::create_dir_all(staging).map_err(|e| format!("{}: {e}", staging.display()))?;
            staging.join(&asset.name)
        }
    };
    let cleanup = |e: String| {
        let _ = std::fs::remove_file(&part);
        e
    };
    progress.total.store(asset.size, Ordering::Relaxed);
    let report = |done: u64, total: Option<u64>| {
        progress.done.store(done, Ordering::Relaxed);
        if let Some(t) = total {
            progress.total.store(t, Ordering::Relaxed);
        }
        !progress.cancel.load(Ordering::Relaxed) && done <= MAX_PACKAGE
    };
    (net.download)(&asset.url, &part, &report).map_err(|e| cleanup(format!("{}: {e}", asset.name)))?;
    if progress.cancel.load(Ordering::Relaxed) {
        return Err(cleanup("cancelled".into()));
    }
    let actual = sha256_file(&part).map_err(cleanup)?;
    if actual != expected {
        return Err(cleanup(format!("{} doesn't match {SUMS} (got {actual}, expected {expected}); it was deleted", asset.name)));
    }
    let outcome = apply(method, &part).map_err(cleanup)?;
    Ok(match (method, outcome) {
        // Zsync-based tools (Gearlever, AppImageUpdate) find the update data by the release's file name.
        (Method::AppImage(_), Outcome::Restart(placed)) => {
            let named = release_named(&placed, release.version(), arch);
            // A failed rename leaves the new AppImage under its old name: it still works.
            Outcome::Restart(if named != placed && std::fs::rename(&placed, &named).is_ok() { named } else { placed })
        }
        (_, outcome) => outcome,
    })
}

/// Where an AppImage called `path` goes once it holds `version`: if its name is a release name
/// (`solidcraft-<version>-linux-<arch>.AppImage`, or the old `solvecraft-` one), the new release's name
/// in the same folder; any other name (renamed by the user, or without a version) is kept.
pub fn release_named(path: &Path, version: &str, arch: &str) -> PathBuf {
    let name = path.file_name().map(|n| n.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    let rest = name.strip_prefix("solidcraft-").or_else(|| name.strip_prefix("solvecraft-"));
    if rest.is_some_and(|r| r.starts_with(|c: char| c.is_ascii_digit()) && r.contains("-linux-") && r.ends_with(".appimage")) {
        path.with_file_name(format!("solidcraft-{}-linux-{arch}.AppImage", version.trim_start_matches('v')))
    } else {
        path.to_path_buf()
    }
}

/// Put a verified package in place.
pub fn apply(method: &Method, package: &Path) -> Result<Outcome, String> {
    match method {
        Method::AppImage(target) => {
            make_executable(package)?;
            std::fs::rename(package, target).map_err(|e| format!("replacing {}: {e}", target.display()))?;
            Ok(Outcome::Restart(target.clone()))
        }
        Method::Exe(exe) => {
            swap(exe, package)?;
            Ok(Outcome::Restart(exe.clone()))
        }
        Method::PortableZip(exe) => {
            let dir = exe.parent().ok_or_else(|| format!("{} has no folder", exe.display()))?;
            let name = exe.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "solidcraft.exe".into());
            let files = [name.as_str(), "solidcraft-cli.exe"];
            let mut extracted = Vec::new();
            for file in files {
                let to = dir.join(format!("{file}.update"));
                match unzip_one(package, file, &to) {
                    Ok(true) => extracted.push((dir.join(file), to)),
                    Ok(false) if file == name => return Err(format!("the portable zip has no {file}")),
                    Ok(false) => {}
                    Err(e) => {
                        for (_, t) in &extracted {
                            let _ = std::fs::remove_file(t);
                        }
                        return Err(e);
                    }
                }
            }
            let _ = std::fs::remove_file(package);
            for (target, new) in &extracted {
                swap(target, new)?;
            }
            Ok(Outcome::Restart(exe.clone()))
        }
        Method::Msi => Ok(Outcome::Installer(PathBuf::from("msiexec"), vec!["/i".into(), package.to_string_lossy().to_string(), "/passive".into()])),
        Method::Dmg => Ok(Outcome::Opened(package.to_path_buf())),
        Method::Tarball(prefix) => apply_tarball(prefix, package),
    }
}

/// Unpack a verified release `.tar.gz` over `prefix`. The archive's top folder is stripped.
/// The previous tree is renamed to `<name>.old` and removed at the next start, so a running
/// program keeps its files until then. A failure puts the previous tree back.
fn apply_tarball(prefix: &Path, package: &Path) -> Result<Outcome, String> {
    let parent = prefix.parent().ok_or_else(|| format!("{} has no folder", prefix.display()))?;
    let name = prefix
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|n| !n.is_empty() && *n != "." && *n != "..")
        .ok_or_else(|| format!("{} has no name", prefix.display()))?;
    let staging = parent.join(format!("{name}.update-new"));
    let backup = parent.join(format!("{name}.old"));
    if staging.exists() {
        std::fs::remove_dir_all(&staging).map_err(|e| format!("clearing {}: {e}", staging.display()))?;
    }
    if let Err(error) = unpack_release_tarball(package, &staging) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(error);
    }
    let relative = if staging.join("bin").join("solidcraft").is_file() {
        PathBuf::from("bin").join("solidcraft")
    } else if staging.join("solidcraft").is_file() {
        PathBuf::from("solidcraft")
    } else {
        let _ = std::fs::remove_dir_all(&staging);
        return Err("the archive has no solidcraft program".into());
    };
    for bin in ["bin/solidcraft", "bin/solidcraft-cli", "bin/solvecraft-cli", "solidcraft"] {
        let path = staging.join(bin);
        if path.is_file() {
            let _ = make_executable(&path);
        }
    }
    if backup.exists() {
        std::fs::remove_dir_all(&backup).map_err(|e| {
            let _ = std::fs::remove_dir_all(&staging);
            format!("removing the previous {}: {e}", backup.display())
        })?;
    }
    if let Err(error) = std::fs::rename(prefix, &backup) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(format!("moving {} aside: {error}", prefix.display()));
    }
    if let Err(error) = std::fs::rename(&staging, prefix) {
        let restored = std::fs::rename(&backup, prefix);
        let _ = std::fs::remove_dir_all(&staging);
        if let Err(restore) = restored {
            return Err(format!("putting the new {} in place: {error}; restoring the previous copy also failed: {restore}", prefix.display()));
        }
        return Err(format!("putting the new {} in place: {error}", prefix.display()));
    }
    let _ = std::fs::remove_file(package);
    Ok(Outcome::Restart(prefix.join(relative)))
}

/// Extract a release `.tar.gz` into `dest`, dropping the single top folder the release wraps
/// around `bin/` and `share/`. Absolute paths, `..` and links are refused.
fn unpack_release_tarball(package: &Path, dest: &Path) -> Result<(), String> {
    let file = std::fs::File::open(package).map_err(|e| format!("{}: {e}", package.display()))?;
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(file));
    std::fs::create_dir_all(dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    let mut total = 0u64;
    let entries = archive.entries().map_err(|e| format!("{}: {e}", package.display()))?;
    for entry in entries {
        let mut entry = entry.map_err(|e| format!("{}: {e}", package.display()))?;
        let path = entry.path().map_err(|_| "unsafe path in archive".to_string())?.into_owned();
        let mut components = path.components();
        match components.next() {
            Some(std::path::Component::Normal(_)) => {}
            Some(_) => return Err(format!("unsafe path in archive: {}", path.display())),
            None => continue,
        }
        let relative: PathBuf = components.collect();
        if relative.as_os_str().is_empty() {
            continue;
        }
        if relative.components().any(|c| !matches!(c, std::path::Component::Normal(_))) {
            return Err(format!("unsafe path in archive: {}", path.display()));
        }
        let target = dest.join(&relative);
        match entry.header().entry_type() {
            tar::EntryType::Directory => {
                std::fs::create_dir_all(&target).map_err(|e| format!("{}: {e}", target.display()))?;
            }
            tar::EntryType::Regular => {
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
                }
                let claimed = entry.header().size().unwrap_or(0);
                if claimed > MAX_UNZIPPED {
                    return Err(format!("{} in the archive is too large", relative.display()));
                }
                #[cfg(unix)]
                let mode = entry.header().mode().unwrap_or(0o644);
                let mut out = std::fs::File::create(&target).map_err(|e| format!("{}: {e}", target.display()))?;
                let copied = std::io::copy(&mut (&mut entry).take(MAX_UNZIPPED + 1), &mut out).map_err(|e| format!("{}: {e}", target.display()))?;
                out.flush().map_err(|e| format!("{}: {e}", target.display()))?;
                if copied > MAX_UNZIPPED {
                    let _ = std::fs::remove_file(&target);
                    return Err(format!("{} in the archive is too large", relative.display()));
                }
                total = total.saturating_add(copied);
                if total > MAX_PACKAGE {
                    return Err("the unpacked archive is too large".into());
                }
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(&target, std::fs::Permissions::from_mode(mode & 0o777));
                }
            }
            _ => return Err(format!("refusing {} in the archive", path.display())),
        }
    }
    Ok(())
}

/// `path` with `suffix` appended to its file name, in the same folder.
fn sibling(path: &Path, suffix: &str) -> Result<PathBuf, String> {
    let name = path.file_name().ok_or_else(|| format!("{} has no file name", path.display()))?;
    Ok(path.with_file_name(format!("{}{suffix}", name.to_string_lossy())))
}

/// Replace `target` (possibly the running executable) with `new`: `target` becomes
/// `target.old`, `new` takes its name; on failure the old file is put back.
pub fn swap(target: &Path, new: &Path) -> Result<(), String> {
    let old = sibling(target, ".old")?;
    let _ = std::fs::remove_file(&old);
    let had = target.exists();
    if had {
        std::fs::rename(target, &old).map_err(|e| format!("moving {} aside: {e}", target.display()))?;
    }
    if let Err(e) = std::fs::rename(new, target) {
        if had {
            let _ = std::fs::rename(&old, target);
        }
        return Err(format!("putting the new {} in place: {e}", target.display()));
    }
    // Not running: no need to keep the old one (the running exe's `.old` goes at next start).
    let _ = std::fs::remove_file(&old);
    Ok(())
}

/// Copy the zip entry whose file name is `name` (in any folder) to `to`. False: no such entry.
fn unzip_one(zip: &Path, name: &str, to: &Path) -> Result<bool, String> {
    let file = std::fs::File::open(zip).map_err(|e| format!("{}: {e}", zip.display()))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("{}: {e}", zip.display()))?;
    let index = (0..archive.len()).find(|&i| {
        archive
            .name_for_index(i)
            .and_then(Result::ok)
            .is_some_and(|n| n.rsplit(['/', '\\']).next().is_some_and(|f| f.eq_ignore_ascii_case(name)) && !n.ends_with('/'))
    });
    let Some(index) = index else { return Ok(false) };
    let mut entry = archive.by_index(index).map_err(|e| format!("{}: {e}", zip.display()))?;
    if entry.size() > MAX_UNZIPPED {
        return Err(format!("{name} in the zip is too large"));
    }
    // The file is closed at the end of this block (before it may be deleted below).
    let copied = {
        let mut out = std::fs::File::create(to).map_err(|e| format!("{}: {e}", to.display()))?;
        let copied = std::io::copy(&mut (&mut entry).take(MAX_UNZIPPED + 1), &mut out).map_err(|e| format!("{}: {e}", to.display()))?;
        out.flush().map_err(|e| format!("{}: {e}", to.display()))?;
        copied
    };
    if copied > MAX_UNZIPPED {
        let _ = std::fs::remove_file(to);
        return Err(format!("{name} in the zip is too large"));
    }
    Ok(true)
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(not(unix))]
fn make_executable(_: &Path) -> Result<(), String> {
    Ok(())
}

/// At start: delete what a previous update moved aside (`solidcraft.exe.old`, or the previous
/// `.tar.gz` folder kept as `<install>.old`).
pub fn clean_up_after_update(exe: Option<&Path>) {
    for file in [exe.map(Path::to_path_buf), exe.and_then(|e| e.parent()).map(|d| d.join("solidcraft-cli.exe"))].into_iter().flatten() {
        if let Ok(old) = sibling(&file, ".old")
            && old.exists()
        {
            let _ = std::fs::remove_file(old);
        }
    }
    // Only the Linux archive's program is named `solidcraft` with no extension. An AppImage or
    // `solidcraft.exe` keeps its own `.old` file, handled above, and its folders are left alone.
    let Some(exe) = exe.filter(|path| path.file_name().is_some_and(|name| name == "solidcraft")) else { return };
    let Some(prefix) = installable_tarball_prefix(exe) else { return };
    let Some(name) = prefix.file_name().and_then(|n| n.to_str()) else { return };
    let Some(backup) = prefix.parent().map(|parent| parent.join(format!("{name}.old"))) else { return };
    // Only a folder this updater left behind: it holds the program, and nothing else is removed.
    if backup.join("bin").join("solidcraft").is_file() || backup.join("solidcraft").is_file() {
        let _ = std::fs::remove_dir_all(backup);
    }
}
