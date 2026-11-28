//! What the model cache is keyed by. `SOLIDCRAFT_EVALUATION_SOURCE`: the digest of the source a
//! design's model is computed from: the `doc` crate and every workspace crate it is built from,
//! the vendored kernel, the patched dependencies, the locked versions of all others, the
//! workspace's build settings and the cache's own reader. A cache is only the result of evaluating
//! its design when that source computed it, and an update that changed none of it (only the
//! interface, the commands or the other file formats, or the version number) computes the same
//! model. Without `.git` (Docker, Nix or a source archive), the same inputs are hashed from disk;
//! `"unknown"` only when part of that source cannot be read, which never trusts a cache.
//! `SOLIDCRAFT_SOURCE_COMMIT`: the commit, recorded with the cache (`"unknown"` without `git`).
//!
//! A git working tree with uncommitted changes has a digest of its own, of those changes: the
//! trees hashed are the *committed* ones, so without them a build from a changed tree would carry
//! the digest of the commit it sits on, and could write a cache that a clean build of that commit
//! (or a build with other changes) trusts. The build script reruns when a file under
//! [`watched`] changes, so the digest follows the working tree in an incremental build too.
//!
//! A Rust file is hashed as [`rust_code`] gives it: without its comments, doc comments included,
//! and with its spacing folded. An edit to comments alone computes the same model, so it keeps the
//! digest and every saved model cache (#23132); any other edit, string literals included, moves it.
//! Every other file is hashed byte for byte.

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The workspace crates a design's model is computed from: `doc` and every workspace crate it
/// depends on, except the display ones (a test checks the list against their manifests).
///
/// `viewport` and `render` are deliberately not keyed. The cache stores the kernel-encoded bodies
/// and the placement shares, never a display plan: the plans are derived on open from the decoded
/// bodies, so a change to either crate cannot change a cached model. Keying them made every
/// interface change rebuild every design (25 s on grid100) for nothing;
/// `a_model_is_computed_without_the_display_crates` keeps doc's model computation from naming them.
pub(crate) const EVALUATION_CRATES: &[&str] =
    &["color", "geom", "profile", "smlib", "sketch", "kernel", "kernel-api", "derive", "develop", "doc", "format"];
/// The other files the saved model depends on, relative to the workspace root.
pub(crate) const EVALUATION_FILES: &[&str] = &["crates/io/build.rs", "crates/io/src/design/native_cache.rs", ".cargo/config.toml"];
/// The committed trees whose every file computes models, hashed as one tree id each: the vendored
/// kernel, so any change under `solidcore/` invalidates the models the old kernel computed.
pub(crate) const EVALUATION_TREES: &[&str] = &["solidcore"];
/// What under `vendor/` does not compute models: notes, and the interface's renderer. The kernel is
/// not under `vendor/`; it is `solidcore/`, in [`EVALUATION_TREES`].
pub(crate) const VENDOR_OUTSIDE: &[&str] = &["README.md", "egui-wgpu"];
/// What under `solidcore/` is not compiled into the kernel: the site, its plans and reports, its
/// fixtures and the tests and goldens that read them, and the rest of its repository's own files.
/// A commit there computes the same model, so it does not invalidate a cache. Matched against the
/// path's first segment, or as a directory anywhere in it ([`DIRECTORY_OUTSIDE`]).
const SOLIDCORE_OUTSIDE: &[&str] = &["docs", "plan", "fixtures", "harness", "scripts", "site", "reports", "bench", ".github", "assets"];
/// Test-only directories under a crate: `tests/`, `benches/`, `examples/` and the goldens' own
/// crate. They are never built into a design's model.
const DIRECTORY_OUTSIDE: &[&str] = &["tests", "benches", "examples", "goldens"];

fn evaluation_paths() -> Vec<String> {
    let mut paths: Vec<String> = EVALUATION_CRATES.iter().map(|c| format!("crates/{c}")).collect();
    paths.extend(EVALUATION_TREES.iter().map(|t| (*t).to_string()));
    paths.extend(EVALUATION_FILES.iter().map(|f| (*f).to_string()));
    paths
}

/// Whether `file` is Rust source, hashed as [`rust_code`] gives it.
fn is_rust(file: &str) -> bool {
    file.ends_with(".rs")
}

/// The length in bytes of the UTF-8 character that starts with `lead`.
fn utf8_len(lead: u8) -> usize {
    match lead {
        0..0x80 => 1,
        0x80..0xE0 => 2,
        0xE0..0xF0 => 3,
        _ => 4,
    }
}

/// Where a raw string literal (`r"…"`, `r#"…"#`, `br"…"`, `cr"…"`) that starts at `at` ends, or
/// `None` when none starts there (`r#type`, a raw identifier, is not one).
fn raw_string_end(b: &[u8], at: usize) -> Option<usize> {
    let mut j = at;
    if matches!(b.get(j), Some(b'b' | b'c')) {
        j += 1;
    }
    if b.get(j) != Some(&b'r') {
        return None;
    }
    j += 1;
    let hashes = b[j..].iter().take_while(|&&c| c == b'#').count();
    j += hashes;
    if b.get(j) != Some(&b'"') {
        return None;
    }
    j += 1;
    while j < b.len() {
        if b[j] == b'"' && b[j + 1..].iter().take(hashes).filter(|&&c| c == b'#').count() == hashes {
            return Some((j + 1 + hashes).min(b.len()));
        }
        j += 1;
    }
    Some(b.len())
}

/// Where a character literal (`'a'`, `'\n'`, `'\u{2014}'`) that starts at `at` ends, or `None`
/// when the quote starts a lifetime or a label (`'a`, `'outer:`).
fn char_end(b: &[u8], at: usize) -> Option<usize> {
    let next = *b.get(at + 1)?;
    if next == b'\\' {
        // An escape: the quote that closes it comes within the longest one, `\u{10FFFF}`.
        let start = at + 3;
        return b.get(start..).and_then(|rest| rest.iter().take(10).position(|&c| c == b'\'')).map(|p| start + p + 1);
    }
    let close = at + 1 + utf8_len(next);
    (b.get(close) == Some(&b'\'')).then_some(close + 1)
}

/// Rust source as the digest sees it (#23132): comments removed (`//`, `/* */` nested, and the doc
/// comments `///`, `//!`, `/** */`), and each run of spacing outside a literal, a removed comment
/// included, folded to one space. String, raw string, byte and character literals are kept as they
/// are, so a `//` inside a string is code. Two files that differ only in their comments or spacing
/// give the same text; a change to anything else changes it.
pub(crate) fn rust_code(src: &str) -> String {
    let b = src.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut space = false;
    let keep = |out: &mut Vec<u8>, space: &mut bool, bytes: &[u8]| {
        if std::mem::take(space) && !out.is_empty() {
            out.push(b' ');
        }
        out.extend_from_slice(bytes);
    };
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_whitespace() {
            space = true;
            i += 1;
        } else if c == b'/' && b.get(i + 1) == Some(&b'/') {
            i += b[i..].iter().position(|&c| c == b'\n').unwrap_or(b.len() - i);
            space = true;
        } else if c == b'/' && b.get(i + 1) == Some(&b'*') {
            let mut depth = 0usize;
            while i < b.len() {
                if b[i] == b'/' && b.get(i + 1) == Some(&b'*') {
                    depth += 1;
                    i += 2;
                } else if b[i] == b'*' && b.get(i + 1) == Some(&b'/') {
                    depth = depth.saturating_sub(1);
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    i += 1;
                }
            }
            space = true;
        } else if let Some(end) =
            (i == 0 || !(b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_' || b[i - 1] >= 0x80)).then(|| raw_string_end(b, i)).flatten()
        {
            keep(&mut out, &mut space, &b[i..end]);
            i = end;
        } else if c == b'"' {
            let start = i;
            i += 1;
            while i < b.len() {
                match b[i] {
                    b'\\' => i += 2,
                    b'"' => {
                        i += 1;
                        break;
                    }
                    _ => i += 1,
                }
            }
            i = i.min(b.len());
            keep(&mut out, &mut space, &b[start..i]);
        } else if let Some(end) = (c == b'\'').then(|| char_end(b, i)).flatten() {
            keep(&mut out, &mut space, &b[i..end]);
            i = end;
        } else {
            let end = (i + utf8_len(c)).min(b.len());
            keep(&mut out, &mut space, &b[i..end]);
            i = end;
        }
    }
    // Every cut above falls on an ASCII byte, so the text stays UTF-8; the source itself if not.
    String::from_utf8(out).unwrap_or_else(|_| src.to_string())
}

/// What a file adds to the digest: a Rust file as [`rust_code`] gives it, any other byte for byte.
fn hashed_content(file: &str, bytes: Vec<u8>) -> Vec<u8> {
    match std::str::from_utf8(&bytes) {
        Ok(text) if is_rust(file) => rust_code(text).into_bytes(),
        _ => bytes,
    }
}

/// The contents of the blobs `ids`, in their order, read through one `git cat-file --batch`;
/// `None` when git fails or one is missing.
fn blobs(root: &Path, ids: &[&str]) -> Option<Vec<Vec<u8>>> {
    use std::io::Write;
    if ids.is_empty() {
        return Some(Vec::new());
    }
    let mut child = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["cat-file", "--batch"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdin = child.stdin.take()?;
    let input = ids.join("\n") + "\n";
    // Written from another thread: git answers while it reads, and a full pipe would stop both.
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()));
    let out = child.wait_with_output().ok()?;
    writer.join().ok()?.ok()?;
    if !out.status.success() {
        return None;
    }
    // Each answer is "<id> <type> <size>\n<content>\n".
    let mut rest = out.stdout.as_slice();
    let mut contents = Vec::with_capacity(ids.len());
    for _ in ids {
        let header_end = rest.iter().position(|&c| c == b'\n')?;
        let header = std::str::from_utf8(&rest[..header_end]).ok()?;
        let size: usize = header.split(' ').nth(2)?.parse().ok()?;
        let start = header_end + 1;
        contents.push(rest.get(start..start + size)?.to_vec());
        rest = rest.get(start + size + 1..).unwrap_or_default();
    }
    Some(contents)
}

fn git(root: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git").arg("-C").arg(root).args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned()).filter(|s| !s.trim().is_empty())
}

/// `Cargo.lock` without the workspace's own packages: their version is the application's, which
/// every release changes; the source they are built from is hashed itself.
fn locked_dependencies(lock: &str) -> String {
    lock.replace("\r\n", "\n")
        .split("\n[[package]]")
        .filter(|block| block.lines().any(|l| l.starts_with("source = ")))
        .collect::<Vec<_>>()
        .join("\n[[package]]")
}

/// The workspace `Cargo.toml` without the application's version.
fn workspace_settings(manifest: &str) -> String {
    let mut section = "";
    let mut kept = Vec::new();
    for line in manifest.lines() {
        if line.starts_with('[') {
            section = line.trim();
        }
        if !(section == "[workspace.package]" && line.trim_start().starts_with("version")) {
            kept.push(line);
        }
    }
    kept.join("\n")
}

/// Whether `path` — after `solidcore/` — is compiled into the kernel: the tests, the goldens'
/// fixtures and the repository's own files are not.
fn solidcore_evaluated(path: &str) -> bool {
    let first = path.split('/').next().unwrap_or(path);
    if SOLIDCORE_OUTSIDE.contains(&first) {
        return false;
    }
    let mut dirs = path.split('/');
    dirs.next();
    dirs.next_back();
    !dirs.any(|d| DIRECTORY_OUTSIDE.contains(&d))
}

/// What in the working tree computes models, relative to the workspace root: the build script
/// reruns when any of it changes, and its uncommitted changes are part of the git digest.
pub(crate) fn watched(root: &Path) -> Vec<String> {
    let mut paths: Vec<String> = Vec::new();
    for c in EVALUATION_CRATES {
        let crate_path = root.join(format!("crates/{c}"));
        if let Some(entries) = sorted_dir(&crate_path) {
            let mut added = false;
            for (name, _) in entries {
                if !name.starts_with('.') && !DIRECTORY_OUTSIDE.contains(&name.as_str()) {
                    paths.push(format!("crates/{c}/{name}"));
                    added = true;
                }
            }
            if !added {
                paths.push(format!("crates/{c}"));
            }
        } else {
            paths.push(format!("crates/{c}"));
        }
    }
    paths.extend(EVALUATION_FILES.iter().map(|f| (*f).to_string()));

    let solidcore_crates = root.join("solidcore/crates");
    if let Some(crates) = sorted_dir(&solidcore_crates) {
        let mut added = false;
        for (krate, krate_path) in crates {
            if krate.starts_with('.') || DIRECTORY_OUTSIDE.contains(&krate.as_str()) {
                continue;
            }
            if let Some(sub_entries) = sorted_dir(&krate_path) {
                for (name, _) in sub_entries {
                    if !name.starts_with('.') && !DIRECTORY_OUTSIDE.contains(&name.as_str()) {
                        paths.push(format!("solidcore/crates/{krate}/{name}"));
                        added = true;
                    }
                }
            }
        }
        if !added {
            paths.push("solidcore/crates".into());
        }
    } else {
        paths.push("solidcore/crates".into());
    }

    paths.extend(["solidcore/Cargo.toml", "solidcore/Cargo.lock", "vendor", "Cargo.lock", "Cargo.toml"].map(String::from));
    paths.sort();
    paths.dedup();
    paths
}

/// `git` in `root`, its raw output even when empty; `None` when it fails.
fn git_bytes(root: &Path, args: &[&str]) -> Option<Vec<u8>> {
    let out = Command::new("git").arg("-C").arg(root).args(args).output().ok()?;
    out.status.success().then_some(out.stdout)
}

/// Add the working tree's own changes under [`watched`] to `digest`: each tracked file that
/// differs from the commit, with its content (or as deleted), and every untracked, not ignored file
/// with its content (a new module is an untracked file until it is added). A Rust file whose
/// change is only to comments or spacing ([`rust_code`]) adds nothing, as a commit of it would
/// not. Nothing is added for a clean tree, so it keeps the commit's digest; `None` when git cannot
/// say.
fn working_changes(root: &Path, digest: &mut Sha256) -> Option<()> {
    let paths = watched(root);
    let mut diff = vec!["diff", "HEAD", "--name-only", "-z", "--no-renames", "--"];
    diff.extend(paths.iter().map(String::as_str));
    let changed = git_bytes(root, &diff)?;
    let mut others = vec!["ls-files", "--others", "--exclude-standard", "-z", "--"];
    others.extend(paths.iter().map(String::as_str));
    let untracked = git_bytes(root, &others)?;
    for file in changed.split(|&b| b == 0).filter(|f| !f.is_empty()) {
        let path = String::from_utf8_lossy(file);
        let now = std::fs::read(root.join(path.as_ref())).ok();
        if is_rust(&path) {
            let then = git_bytes(root, &["show", &format!("HEAD:./{path}")]);
            let code = |bytes: Option<Vec<u8>>| bytes.map(|b| hashed_content(&path, b));
            if code(now.clone()) == code(then) {
                continue;
            }
        }
        digest.update(format!("working {path}\n"));
        match now {
            Some(bytes) => digest.update(hashed_content(&path, bytes)),
            None => digest.update(b"deleted\n"),
        }
    }
    for file in untracked.split(|&b| b == 0).filter(|f| !f.is_empty()) {
        let path = String::from_utf8_lossy(file);
        digest.update(format!("untracked {path}\n"));
        digest.update(hashed_content(&path, std::fs::read(root.join(path.as_ref())).unwrap_or_default()));
    }
    Some(())
}

fn hex_digest(digest: Sha256) -> String {
    digest.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// The digest of the committed source of the saved model (see the module notes), or `None` when
/// `.git` is absent, `git` is unavailable, or a part of the source is not in the commit.
pub(crate) fn evaluation_source(root: &Path) -> Option<String> {
    if !root.join(".git").exists() {
        return None;
    }
    let paths = evaluation_paths();
    let specs: Vec<String> = paths.iter().map(|p| format!("HEAD:./{p}")).collect();
    let mut args = vec!["rev-parse"];
    args.extend(specs.iter().map(String::as_str));
    let ids = git(root, &args)?;
    if ids.lines().count() != paths.len() {
        return None;
    }
    let mut digest = Sha256::new();
    // Every file under each path, "<mode> <type> <id>\t<path from the root>", NUL-separated so a
    // name git would quote (non-ASCII, a tab) is listed as it is.
    let mut listing = vec!["ls-tree", "-r", "-z", "HEAD", "--"];
    let specs: Vec<String> = paths.iter().map(|p| format!("./{p}")).chain(["./vendor/".to_string()]).collect();
    listing.extend(specs.iter().map(String::as_str));
    let listing = git_bytes(root, &listing)?;
    let mut files: Vec<(String, Option<(String, String)>)> = Vec::new();
    for record in listing.split(|&b| b == 0).filter(|r| !r.is_empty()) {
        let record = String::from_utf8_lossy(record).into_owned();
        // Only a file's content is read; a submodule's entry is keyed by its commit id.
        let parsed = record.split_once('\t').and_then(|(meta, file)| {
            let mut meta = meta.split_whitespace();
            let (kind, id) = (meta.nth(1)?, meta.next()?);
            Some((id.to_string(), if kind == "blob" { file.to_string() } else { format!("{file} {kind}") }))
        });
        // The vendored kernel's tree: every file that computes models, not the repository's own;
        // `vendor/`: every entry but the notes and the interface's renderer.
        let outside = parsed.as_ref().is_some_and(|(_, file)| {
            EVALUATION_TREES.iter().any(|t| file.strip_prefix(&format!("{t}/")).is_some_and(|rest| !solidcore_evaluated(rest)))
                || file.strip_prefix("vendor/").is_some_and(|rest| VENDOR_OUTSIDE.contains(&rest.split('/').next().unwrap_or(rest)))
        });
        if !outside {
            files.push((record, parsed));
        }
    }
    let rust_ids: Vec<&str> =
        files.iter().filter_map(|(_, parsed)| parsed.as_ref()).filter(|(_, file)| is_rust(file)).map(|(id, _)| id.as_str()).collect();
    let mut rust = blobs(root, &rust_ids)?.into_iter();
    for (record, parsed) in files {
        match parsed {
            Some((_, file)) if is_rust(&file) => {
                let code = hashed_content(&file, rust.next()?);
                digest.update(format!("{file} rust {}\n", code.len()));
                digest.update(code);
            }
            Some((id, file)) => digest.update(format!("{file} {id}\n")),
            // A record that cannot be read is hashed whole rather than left out.
            None => digest.update(format!("record {record}\n")),
        }
    }
    digest.update(locked_dependencies(&git(root, &["show", "HEAD:./Cargo.lock"])?));
    digest.update(workspace_settings(&git(root, &["show", "HEAD:./Cargo.toml"])?));
    // The committed trees above are the commit's; a working tree with changes of its own computes
    // a model this digest would otherwise say is the commit's (#7168's own hazard).
    working_changes(root, &mut digest)?;
    Some(hex_digest(digest))
}

/// What the disk hash leaves out: VCS and OS litter, editor backups, and cargo's build output,
/// which is a directory named `target` or `target-<name>` (one per worktree here). A file, or a
/// directory, whose name only starts with `target` (`target_loops.json`, `target_frame.rs`) is
/// source and is hashed.
fn skip_disk_name(name: &str, is_dir: bool) -> bool {
    name == ".git"
        || name == ".DS_Store"
        || name == "__pycache__"
        || (is_dir && (name == "target" || name.starts_with("target-")))
        || name.ends_with('~')
        || name.ends_with(".swp")
}

/// Whether `path` is a directory itself (a symlink to one is hashed as a link, not walked).
fn is_real_dir(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.is_dir())
}

fn sorted_dir(path: &Path) -> Option<Vec<(String, PathBuf)>> {
    let mut entries = std::fs::read_dir(path).ok()?.collect::<Result<Vec<_>, _>>().ok()?;
    entries.sort_by_key(|e| e.file_name());
    entries.into_iter().map(|e| Some((e.file_name().into_string().ok()?, e.path()))).collect()
}

fn hash_disk_path(root: &Path, path: &Path, digest: &mut Sha256, watch: bool) -> Option<()> {
    let rel = path.strip_prefix(root).ok()?.to_str()?.replace('\\', "/");
    let meta = std::fs::symlink_metadata(path).ok()?;
    if watch {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    if meta.is_dir() {
        digest.update(format!("dir {}\n{rel}\n", rel.len()));
        for (name, child) in sorted_dir(path)? {
            if !skip_disk_name(&name, is_real_dir(&child)) {
                hash_disk_path(root, &child, digest, watch)?;
            }
        }
    } else {
        let raw = if meta.file_type().is_symlink() {
            std::fs::read_link(path).ok()?.to_str()?.replace('\\', "/").into_bytes()
        } else if meta.is_file() {
            std::fs::read(path).ok()?
        } else {
            return None;
        };
        let bytes = match std::str::from_utf8(&raw) {
            Ok(text) if text.contains("\r\n") => text.replace("\r\n", "\n").into_bytes(),
            _ => raw,
        };
        let bytes = if meta.is_file() { hashed_content(&rel, bytes) } else { bytes };
        digest.update(format!("file {} {}\n{rel}\n", rel.len(), bytes.len()));
        digest.update(&bytes);
    }
    Some(())
}

/// Fallback when `.git` or `git` is unavailable: hash the same evaluation inputs from `root`.
pub(crate) fn evaluation_source_from_disk(root: &Path, watch: bool) -> Option<String> {
    let mut digest = Sha256::new();
    digest.update("disk-v2\n");
    for rel in evaluation_paths() {
        hash_disk_path(root, &root.join(rel), &mut digest, watch)?;
    }
    let vendor = root.join("vendor");
    if watch {
        println!("cargo:rerun-if-changed={}", vendor.display());
    }
    for (name, child) in sorted_dir(&vendor)? {
        if !VENDOR_OUTSIDE.contains(&name.as_str()) && !skip_disk_name(&name, is_real_dir(&child)) {
            hash_disk_path(root, &child, &mut digest, watch)?;
        }
    }
    for (file, filter) in [("Cargo.lock", locked_dependencies as fn(&str) -> String), ("Cargo.toml", workspace_settings)] {
        let path = root.join(file);
        if watch {
            println!("cargo:rerun-if-changed={}", path.display());
        }
        let text = filter(&std::fs::read_to_string(&path).ok()?);
        digest.update(format!("meta {file} {}\n{text}", text.len()));
    }
    Some(hex_digest(digest))
}

fn main() {
    let crate_dir = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap_or_default());
    let root = crate_dir.join("../..");
    let has_git = root.join(".git").exists();
    let commit = has_git.then(|| git(&crate_dir, &["rev-parse", "HEAD"])).flatten().map(|s| s.trim().to_string()).unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=SOLIDCRAFT_SOURCE_COMMIT={commit}");
    let source = evaluation_source(&root).or_else(|| evaluation_source_from_disk(&root, true)).unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=SOLIDCRAFT_EVALUATION_SOURCE={source}");
    println!("cargo:rustc-env=SOLIDCRAFT_EVALUATION_CRATES={}", EVALUATION_CRATES.join(","));
    // A new commit in this checkout (or worktree) restamps the build.
    if let Some(dir) = has_git.then(|| git(&crate_dir, &["rev-parse", "--git-dir"])).flatten() {
        println!("cargo:rerun-if-changed={}/logs/HEAD", dir.trim());
        // An edit to what computes models restamps it too: the digest carries uncommitted changes.
        for path in watched(&root) {
            println!("cargo:rerun-if-changed={}", root.join(path).display());
        }
    }
    println!("cargo:rerun-if-changed=build.rs");
}
