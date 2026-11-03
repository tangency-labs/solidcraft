//! Preferences › Plugins: packages installed under the per-user data folder.
//!
//! A plugin is a directory of `{data}/plugins/<id>/plugin.json`. The page searches that list and
//! can install one from a GitHub repository link. It reads `plugin.json` and records which ids
//! are turned off. It does not load or run anything from the folder. Running a plugin (the wasm
//! host in docs/plugins.md) is a later step; it will honour `disabled_plugins`.

use std::io::Read;
use std::path::{Path, PathBuf};

use egui::RichText;
use serde::Deserialize;

use crate::SolveApp;

/// How many turned-off ids a preferences file may name.
pub const MAX_DISABLED: usize = 256;
/// `plugin.json` larger than this is refused.
const MAX_MANIFEST_BYTES: u64 = 16 * 1024;

/// One installed plugin, as its manifest describes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plugin {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
}

/// A folder under the plugins directory that is not a plugin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skipped {
    pub folder: String,
    pub reason: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    id: String,
    name: String,
    version: String,
    #[serde(default)]
    description: String,
}

/// The plugins directory: `$XDG_DATA_HOME/solidcraft/plugins` (and the platform equivalents of
/// [`solidcraft_engine::recovery::data_dir`]). `None` when that folder cannot be named.
pub fn plugins_dir() -> Option<PathBuf> {
    solidcraft_engine::recovery::data_dir().map(|d| d.join("plugins"))
}

/// An id a folder and a manifest may share: 1–64 characters, lowercase letters, digits and
/// hyphens, not starting or ending with a hyphen.
pub fn valid_id(id: &str) -> bool {
    let b = id.as_bytes();
    (1..=64).contains(&b.len())
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
        && b[0] != b'-'
        && *b.last().unwrap_or(&b'-') != b'-'
}

fn plain(text: &str, max: usize) -> bool {
    !text.is_empty() && text.chars().count() <= max && text.chars().all(|c| !c.is_control())
}

fn check(m: Manifest, folder: &str) -> Result<Plugin, String> {
    if m.id != folder {
        return Err(format!("plugin.json id `{}` does not match the folder `{folder}`", m.id));
    }
    if !valid_id(&m.id) {
        return Err("the id must be 1–64 lowercase letters, digits and hyphens".into());
    }
    let name = m.name.trim();
    if !plain(name, 80) {
        return Err("the name must be 1–80 characters with no line breaks".into());
    }
    let version = m.version.trim();
    if !plain(version, 32) || !version.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+')) {
        return Err("the version must be 1–32 letters, digits, dots, hyphens or plus signs".into());
    }
    let description = m.description.trim();
    if description.chars().count() > 240 || description.chars().any(|c| c.is_control()) {
        return Err("the description must be at most 240 characters with no line breaks".into());
    }
    Ok(Plugin { id: m.id, name: name.to_string(), version: version.to_string(), description: description.to_string() })
}

fn read_plugin(dir: &Path, folder: &str) -> Result<Plugin, String> {
    let path = dir.join("plugin.json");
    let meta = std::fs::metadata(&path).map_err(|_| "no plugin.json".to_string())?;
    if !meta.is_file() || meta.len() > MAX_MANIFEST_BYTES {
        return Err("plugin.json is missing or larger than 16 KiB".into());
    }
    let text = std::fs::read_to_string(&path).map_err(|e| format!("plugin.json could not be read ({e})"))?;
    let manifest: Manifest = serde_json::from_str(&text).map_err(|e| format!("plugin.json is not a plugin manifest ({e})"))?;
    check(manifest, folder)
}

/// The plugins in `dir`, and the folders that were not. A missing directory is an empty list.
pub fn scan(dir: &Path) -> (Vec<Plugin>, Vec<Skipped>) {
    let mut plugins = Vec::new();
    let mut skipped = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return (plugins, skipped);
    };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else { continue };
        if !kind.is_dir() {
            continue;
        }
        let Some(folder) = entry.file_name().into_string().ok() else {
            skipped.push(Skipped { folder: String::new(), reason: "the folder name is not text".into() });
            continue;
        };
        if folder.starts_with('.') {
            continue;
        }
        match read_plugin(&entry.path(), &folder) {
            Ok(plugin) => plugins.push(plugin),
            Err(reason) => skipped.push(Skipped { folder, reason }),
        }
    }
    plugins.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
    skipped.sort_by(|a, b| a.folder.cmp(&b.folder));
    (plugins, skipped)
}

/// Whether an installed plugin is offered. Unknown ids are on; an id in `disabled_plugins` is off.
pub fn enabled(app: &SolveApp, id: &str) -> bool {
    !app.preferences.disabled_plugins.contains(id)
}

/// A repository SolidCraft can download. Only `github.com`, and only the repository itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GitHubRepo {
    pub owner: String,
    pub repo: String,
    pub git_ref: Option<String>,
}

/// What [`install_zip`] put in the plugins folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Installed {
    pub id: String,
    pub name: String,
    pub updated: bool,
}

/// A downloaded repository larger than this is refused, and again once it is unpacked.
const MAX_ZIP: u64 = 256 * 1024 * 1024;
const MAX_ENTRIES: usize = 4_000;
const MAX_FILE: u64 = 64 * 1024 * 1024;

impl GitHubRepo {
    /// The zip GitHub serves for this repository (the default branch, unless a tag or branch was named).
    pub(crate) fn archive_url(&self) -> String {
        match &self.git_ref {
            Some(git_ref) => format!("https://api.github.com/repos/{}/{}/zipball/{}", self.owner, self.repo, encode_ref(git_ref)),
            None => format!("https://api.github.com/repos/{}/{}/zipball", self.owner, self.repo),
        }
    }

    fn caption(&self) -> String {
        match &self.git_ref {
            Some(git_ref) => format!("{}/{} ({git_ref})", self.owner, self.repo),
            None => format!("{}/{}", self.owner, self.repo),
        }
    }
}

/// `https://github.com/owner/repo`, `git@github.com:owner/repo.git`, or `owner/repo`.
/// A `/tree/<branch>`, `/commit/<sha>` or `/releases/tag/<tag>` link selects that revision.
/// A link to a file or another host is refused.
pub(crate) fn parse_github(input: &str) -> Result<GitHubRepo, String> {
    let text = input.trim();
    if text.is_empty() {
        return Err("Paste a GitHub repository link.".into());
    }
    if text.starts_with("http://") {
        return Err("The link has to start with https://github.com/.".into());
    }
    let (owner, repo, git_ref) = if let Some(rest) = text.strip_prefix("git@github.com:") { split_ssh(rest)? } else { split_web(text)? };
    let repo = repo.strip_suffix(".git").unwrap_or(repo);
    github_owner(owner)?;
    github_repo(repo)?;
    if let Some(git_ref) = &git_ref {
        github_ref(git_ref)?;
    }
    Ok(GitHubRepo { owner: owner.to_string(), repo: repo.to_string(), git_ref })
}

fn split_ssh(rest: &str) -> Result<(&str, &str, Option<String>), String> {
    let rest = rest.split(['?', '#']).next().unwrap_or(rest);
    let mut parts = rest.split('/').filter(|part| !part.is_empty());
    let owner = parts.next().ok_or_else(|| "Paste a link like https://github.com/owner/repository.".to_string())?;
    let repo = parts.next().ok_or_else(|| "Paste a link like https://github.com/owner/repository.".to_string())?;
    if parts.next().is_some() {
        return Err("Paste the repository link, not a file inside it.".into());
    }
    Ok((owner, repo, None))
}

fn split_web(text: &str) -> Result<(&str, &str, Option<String>), String> {
    let bare = text.strip_prefix("https://").unwrap_or(text);
    if bare.contains("://") {
        return Err("The link has to be a github.com repository.".into());
    }
    let bare = bare.split(['?', '#']).next().unwrap_or(bare);
    if bare.split('/').next().unwrap_or("").contains('@') {
        return Err("The link has to be a github.com repository.".into());
    }
    let mut parts: Vec<&str> = bare.split('/').filter(|part| !part.is_empty()).collect();
    let had_host = matches!(parts.first().copied(), Some("github.com" | "www.github.com"));
    if had_host {
        parts.remove(0);
    } else if text.contains("://") || parts.first().is_some_and(|part| part.contains('.')) {
        return Err("The link has to be a github.com repository.".into());
    }
    if parts.len() < 2 {
        return Err("Paste a link like https://github.com/owner/repository.".into());
    }
    let git_ref = match &parts[2..] {
        [] => None,
        ["tree", branch] | ["commit", branch] => Some((*branch).to_string()),
        ["releases", "tag", tag] => Some((*tag).to_string()),
        _ => return Err("Paste the repository link, not a file inside it.".into()),
    };
    Ok((parts[0], parts[1], git_ref))
}

fn github_owner(name: &str) -> Result<(), String> {
    let ok = (1..=39).contains(&name.len())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        && !name.starts_with('-')
        && !name.ends_with('-');
    if ok { Ok(()) } else { Err("That link is not a GitHub repository.".into()) }
}

fn github_repo(name: &str) -> Result<(), String> {
    let ok = (1..=100).contains(&name.len())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        && !name.starts_with('.')
        && !name.ends_with('.')
        && !name.contains("..");
    if ok { Ok(()) } else { Err("That link is not a GitHub repository.".into()) }
}

fn github_ref(name: &str) -> Result<(), String> {
    let ok = (1..=255).contains(&name.len()) && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if ok { Ok(()) } else { Err("That link's branch or tag is not one SolidCraft can request.".into()) }
}

fn encode_ref(git_ref: &str) -> String {
    let mut out = String::new();
    for byte in git_ref.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(byte as char),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn matches_query(plugin: &Plugin, query: &str) -> bool {
    let query = query.trim();
    if query.is_empty() {
        return true;
    }
    let query = query.to_lowercase();
    [plugin.name.as_str(), plugin.id.as_str(), plugin.description.as_str(), plugin.version.as_str()]
        .iter()
        .any(|text| text.to_lowercase().contains(&query))
}

/// Install the one plugin inside a GitHub zipball. The folder that contains `plugin.json` is
/// copied to `plugins/<id>/`. Nothing in the archive is executed. A second install of the same
/// id replaces the folder.
pub(crate) fn install_zip(bytes: &[u8], plugins_dir: &Path) -> Result<Installed, String> {
    if bytes.len() as u64 > MAX_ZIP {
        return Err(format!("That repository is larger than {} MB.", MAX_ZIP / (1024 * 1024)));
    }
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| format!("GitHub did not return a zip archive ({e})"))?;
    if archive.len() > MAX_ENTRIES {
        return Err("That repository has too many files.".into());
    }
    let mut manifests = Vec::new();
    let mut unpacked = 0u64;
    for index in 0..archive.len() {
        let entry = archive.by_index(index).map_err(|e| format!("the archive could not be read ({e})"))?;
        let Some(name) = entry.enclosed_name() else {
            return Err("That archive contains a path that leaves its folder.".into());
        };
        if name.components().any(|component| matches!(component, std::path::Component::ParentDir)) {
            return Err("That archive contains a path that leaves its folder.".into());
        }
        if entry.is_symlink() || !entry.is_file() {
            continue;
        }
        let size = entry.size();
        if size > MAX_FILE {
            return Err("A file in that repository is too large.".into());
        }
        unpacked = unpacked.saturating_add(size);
        if unpacked > MAX_ZIP {
            return Err(format!("That repository unpacks to more than {} MB.", MAX_ZIP / (1024 * 1024)));
        }
        let is_manifest = name.file_name().and_then(|file| file.to_str()) == Some("plugin.json");
        let hidden = name.components().any(|component| component.as_os_str().to_str().is_some_and(|text| text.starts_with('.')));
        if is_manifest && !hidden {
            manifests.push((index, name));
        }
    }
    let (index, manifest_path) = match manifests.as_slice() {
        [one] => one.clone(),
        [] => return Err("That repository has no plugin.json.".into()),
        many => {
            let shown = many.iter().take(4).map(|(_, path)| path.display().to_string()).collect::<Vec<_>>().join(", ");
            return Err(format!("That repository has {} plugin.json files ({shown}). Add the repository that is the one plugin.", many.len()));
        }
    };
    let manifest_bytes = {
        let mut entry = archive.by_index(index).map_err(|e| format!("plugin.json could not be read ({e})"))?;
        if entry.size() > MAX_MANIFEST_BYTES {
            return Err("plugin.json is missing or larger than 16 KiB".into());
        }
        read_limited(&mut entry, MAX_MANIFEST_BYTES)?
    };
    let text = String::from_utf8(manifest_bytes).map_err(|_| "plugin.json is not a plugin manifest (it is not text)".to_string())?;
    let manifest: Manifest = serde_json::from_str(&text).map_err(|e| format!("plugin.json is not a plugin manifest ({e})"))?;
    let id = manifest.id.clone();
    let plugin = check(manifest, &id)?;
    let root = manifest_path.parent().unwrap_or(Path::new("")).to_path_buf();

    std::fs::create_dir_all(plugins_dir).map_err(|e| format!("Could not create the plugins folder: {e}"))?;
    let staging = Staging { path: plugins_dir.join(format!(".{}.incoming", plugin.id)) };
    let _ = std::fs::remove_dir_all(&staging.path);
    std::fs::create_dir_all(&staging.path).map_err(|e| format!("Could not create the plugins folder: {e}"))?;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|e| format!("the archive could not be read ({e})"))?;
        if entry.is_symlink() || !entry.is_file() {
            continue;
        }
        let Some(name) = entry.enclosed_name() else { continue };
        let Ok(relative) = name.strip_prefix(&root) else { continue };
        if relative.as_os_str().is_empty() || relative.components().any(|component| matches!(component, std::path::Component::ParentDir)) {
            continue;
        }
        let dest = staging.path.join(relative);
        if !dest.starts_with(&staging.path) {
            return Err("That archive contains a path that leaves its folder.".into());
        }
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("could not create a folder ({e})"))?;
        }
        let mut file = std::fs::File::create(&dest).map_err(|e| format!("{}: {e}", dest.display()))?;
        let copied = std::io::copy(&mut (&mut entry).take(MAX_FILE + 1), &mut file).map_err(|e| format!("{}: {e}", dest.display()))?;
        if copied > MAX_FILE {
            return Err("A file in that repository is too large.".into());
        }
    }
    let verified = read_plugin(&staging.path, &plugin.id)?;
    let dest = plugins_dir.join(&verified.id);
    if dest.parent() != Some(plugins_dir) {
        return Err("the plugin id is not a folder name".into());
    }
    let updated = dest.exists();
    place(&staging.path, &dest)?;
    Ok(Installed { id: verified.id, name: verified.name, updated })
}

fn read_limited(entry: &mut impl Read, limit: u64) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    entry.take(limit + 1).read_to_end(&mut bytes).map_err(|e| format!("plugin.json could not be read ({e})"))?;
    if bytes.len() as u64 > limit {
        return Err("plugin.json is missing or larger than 16 KiB".into());
    }
    Ok(bytes)
}

/// Removes the staging folder unless it was moved into place.
struct Staging {
    path: PathBuf,
}

impl Drop for Staging {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn place(staging: &Path, dest: &Path) -> Result<(), String> {
    let backup_name = format!(".{}.previous", dest.file_name().and_then(|name| name.to_str()).unwrap_or("plugin"));
    let backup = dest.with_file_name(backup_name);
    let _ = std::fs::remove_dir_all(&backup);
    let had = dest.exists();
    if had {
        std::fs::rename(dest, &backup).map_err(|e| format!("could not replace the installed plugin ({e})"))?;
    }
    if let Err(e) = std::fs::rename(staging, dest) {
        if had {
            let _ = std::fs::rename(&backup, dest);
        }
        return Err(format!("could not put the plugin in place ({e})"));
    }
    if had {
        let _ = std::fs::remove_dir_all(&backup);
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn explain_fetch(message: &str, repo: &GitHubRepo) -> String {
    if message.contains("HTTP 404") { format!("GitHub has no repository {}/{}.", repo.owner, repo.repo) } else { message.to_string() }
}

#[cfg(not(target_arch = "wasm32"))]
fn fetch_and_install(download: &crate::update::install::HttpDownload, repo: &GitHubRepo, dir: &Path) -> Result<Installed, String> {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let tmp = std::env::temp_dir().join(format!("solidcraft-plugin-{}-{nanos}", std::process::id()));
    let too_big = std::sync::atomic::AtomicBool::new(false);
    let downloaded = download(&repo.archive_url(), &tmp, &|done, _| {
        if done > MAX_ZIP {
            too_big.store(true, std::sync::atomic::Ordering::Relaxed);
            false
        } else {
            true
        }
    });
    let result = match downloaded {
        Err(_) if too_big.load(std::sync::atomic::Ordering::Relaxed) => {
            Err(format!("That repository is larger than {} MB.", MAX_ZIP / (1024 * 1024)))
        }
        Err(error) => Err(explain_fetch(&error.to_string(), repo)),
        Ok(()) => match std::fs::read(&tmp) {
            Ok(bytes) => install_zip(&bytes, dir),
            Err(error) => Err(format!("the download could not be read ({error})")),
        },
    };
    let _ = std::fs::remove_file(&tmp);
    result
}

fn poll_install(app: &mut SolveApp) {
    let Some(rx) = &app.prefs_window.plugin_job else { return };
    let result = match rx.try_recv() {
        Ok(result) => result,
        Err(std::sync::mpsc::TryRecvError::Empty) => return,
        Err(std::sync::mpsc::TryRecvError::Disconnected) => Err("Adding the repository stopped.".into()),
    };
    app.prefs_window.plugin_job = None;
    match result {
        Ok(installed) => {
            let message = if installed.updated {
                crate::i18n::format("ui.plugins_updated", &[("name", &installed.name)])
            } else {
                crate::i18n::format("ui.plugins_installed", &[("name", &installed.name)])
            };
            app.prefs_window.plugin_url.clear();
            app.set_status(message, false);
        }
        Err(error) => app.set_status(error, true),
    }
}

fn start_add(app: &mut SolveApp, ctx: &egui::Context) {
    if app.prefs_window.plugin_job.is_some() {
        return;
    }
    let repo = match parse_github(&app.prefs_window.plugin_url) {
        Ok(repo) => repo,
        Err(error) => {
            app.set_status(error, true);
            return;
        }
    };
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (repo, ctx);
        app.set_status(crate::i18n::tr("ui.plugins_desktop").to_string(), true);
        return;
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let Some(download) = app.services.http_download.clone() else {
            app.set_status(crate::i18n::tr("ui.plugins_desktop").to_string(), true);
            return;
        };
        let Some(dir) = plugins_dir() else {
            app.set_status(crate::i18n::tr("ui.plugins_desktop").to_string(), true);
            return;
        };
        if let Err(error) = std::fs::create_dir_all(&dir) {
            app.set_status(format!("Could not create the plugins folder: {error}"), true);
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = ctx.clone();
        let spawned = std::thread::Builder::new().name("plugin-install".into()).spawn(move || {
            let result = fetch_and_install(&download, &repo, &dir);
            let _ = tx.send(result);
            ctx.request_repaint();
        });
        match spawned {
            Ok(_) => {
                app.prefs_window.plugin_job = Some(rx);
                app.set_status(crate::i18n::tr("ui.plugins_adding").to_string(), false);
            }
            Err(error) => app.set_status(format!("Could not add the repository: {error}"), true),
        }
    }
}

/// The Plugins page. Desktop only: the browser build has no per-user plugin folder.
pub fn page(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = crate::theme::Tokens::get();
    if cfg!(target_arch = "wasm32") {
        ui.label(crate::i18n::tr("ui.plugins_desktop"));
        return;
    }
    let Some(dir) = plugins_dir() else {
        ui.label(crate::i18n::tr("ui.plugins_desktop"));
        return;
    };
    let path = dir.display().to_string();
    let searching = crate::prefs_search::querying(ui);
    poll_install(app);
    let busy = app.prefs_window.plugin_job.is_some();
    if busy {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
    }
    // The page's own box is for browsing Plugins. The dialog search is the filter while it is active.
    if !searching {
        crate::ui_kit::search_field(ui, ui.id().with("plugins_search"), &mut app.prefs_window.plugin_query, crate::i18n::tr("ui.plugins_search"));
        ui.add_space(8.0);
        let mut submit = false;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let field_width = (ui.available_width() - 168.0 - 8.0).max(40.0);
            let (enter, _) = ui
                .allocate_ui_with_layout(egui::vec2(field_width, crate::ui_kit::row_height()), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    crate::ui_kit::text_field(ui, &mut app.prefs_window.plugin_url, "https://github.com/owner/repository")
                })
                .inner;
            let add = crate::ui_kit::button(ui, "Add repository", crate::ui_kit::Button::Enabled(!busy));
            crate::scenario::publish_handle("prefs:plugins:add", add.rect.center());
            submit = !busy && (enter || add.clicked());
        });
        if submit {
            start_add(app, ui.ctx());
        }
        if busy {
            ui.add(egui::Label::new(RichText::new(crate::i18n::tr("ui.plugins_adding")).size(t.font_small).color(t.text_dim)).wrap());
        } else if let Ok(repo) = parse_github(&app.prefs_window.plugin_url) {
            ui.add(egui::Label::new(RichText::new(repo.caption()).size(t.font_small).color(t.text_dim)).wrap());
        }
        ui.add(egui::Label::new(RichText::new(crate::i18n::tr("ui.plugins_add_note")).size(t.font_small).color(t.text_dim)).wrap());
        ui.add_space(8.0);
    }
    if searching {
        crate::prefs_search::begin_section(ui, crate::i18n::tr("ui.plugins"));
    } else {
        ui.add(
            egui::Label::new(RichText::new(crate::i18n::format("ui.plugins_note", &[("path", &path)])).size(t.font_small).color(t.text_dim)).wrap(),
        );
    }
    ui.add_space(8.0);
    if !searching && crate::ui_kit::button(ui, crate::i18n::tr_label("Show in folder"), crate::ui_kit::Button::Normal).clicked() {
        // `show_in_folder` opens the parent of the path it is given, so the argument is a name
        // inside the plugins directory (it need not exist).
        if let Err(e) = std::fs::create_dir_all(&dir) {
            app.set_status(format!("Could not create the plugins folder: {e}"), true);
        } else if let Some(show) = &app.services.show_in_folder {
            show(&dir.join("plugin.json").display().to_string());
        }
    }
    ui.add_space(8.0);
    let (plugins, skipped) = scan(&dir);
    let query = app.prefs_window.plugin_query.clone();
    if plugins.is_empty() && !searching {
        ui.label(crate::i18n::tr("ui.plugins_empty"));
    }
    let mut shown = 0usize;
    for plugin in plugins.iter().filter(|plugin| searching || matches_query(plugin, &query)) {
        if searching && !crate::prefs_search::texts_hit(ui, &[&plugin.name, &plugin.id, &plugin.description, &plugin.version]) {
            continue;
        }
        shown += 1;
        if searching {
            crate::prefs_search::reveal(ui);
            crate::prefs_search::push_force(ui);
        }
        let mut on = enabled(app, &plugin.id);
        ui.horizontal(|ui| {
            let check = crate::ui_kit::checkbox(ui, "", &mut on);
            crate::scenario::publish_handle(&format!("prefs:plugin:{}", plugin.id), check.rect.center());
            if check.changed() {
                if on {
                    app.preferences.disabled_plugins.remove(&plugin.id);
                } else if app.preferences.disabled_plugins.len() < MAX_DISABLED {
                    app.preferences.disabled_plugins.insert(plugin.id.clone());
                }
            }
            ui.vertical(|ui| {
                ui.label(RichText::new(&plugin.name).strong());
                ui.label(
                    RichText::new(crate::i18n::format("ui.plugins_version", &[("version", &plugin.version)])).size(t.font_small).color(t.text_dim),
                );
                if !plugin.description.is_empty() {
                    ui.label(RichText::new(&plugin.description).size(t.font_small).color(t.text_dim));
                }
            });
        });
        ui.separator();
        if searching {
            crate::prefs_search::pop_force(ui);
        }
    }
    if !searching && !plugins.is_empty() && shown == 0 {
        ui.label(crate::i18n::tr("ui.plugins_none"));
    }
    if !searching && query.trim().is_empty() && !skipped.is_empty() {
        ui.add_space(6.0);
        ui.label(
            RichText::new(crate::i18n::format("ui.plugins_skipped", &[("count", &skipped.len().to_string())])).size(t.font_small).color(t.text_dim),
        );
        for skip in skipped.iter().take(8) {
            let who = if skip.folder.is_empty() { "folder" } else { skip.folder.as_str() };
            ui.label(RichText::new(format!("{who}: {}", skip.reason)).size(t.font_small).color(t.text_dim));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, folder: &str, body: &str) {
        let d = dir.join(folder);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("plugin.json"), body).unwrap();
    }

    #[test]
    fn a_folder_with_plugin_json_is_listed_and_a_bad_one_is_skipped() {
        let dir = std::env::temp_dir().join(format!("solidcraft-plugins-scan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        write(
            &dir,
            "ftc-frc-parts",
            r#"{"id":"ftc-frc-parts","name":"FTC and FRC parts","version":"0.1.0","description":"Commercial robot parts."}"#,
        );
        write(&dir, "broken", r#"{"id":"other","name":"Nope","version":"1"}"#);
        std::fs::create_dir_all(dir.join("notes")).unwrap();
        let (plugins, skipped) = scan(&dir);
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].id, "ftc-frc-parts");
        assert_eq!(plugins[0].name, "FTC and FRC parts");
        assert!(skipped.iter().any(|s| s.folder == "broken" && s.reason.contains("does not match")));
        assert!(skipped.iter().any(|s| s.folder == "notes" && s.reason.contains("no plugin.json")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ids_and_manifests_refuse_what_would_not_fit_on_the_page() {
        assert!(valid_id("a"));
        assert!(valid_id("ftc-frc-parts"));
        assert!(!valid_id(""));
        assert!(!valid_id("-a"));
        assert!(!valid_id("a-"));
        assert!(!valid_id("FTC"));
        assert!(!valid_id(&"a".repeat(65)));
        let bad = Manifest { id: "gear".into(), name: "Gear\nbox".into(), version: "1".into(), description: String::new() };
        assert!(check(bad, "gear").is_err());
        let big = Manifest { id: "gear".into(), name: "Gear".into(), version: "1".into(), description: "x".repeat(241) };
        assert!(check(big, "gear").unwrap_err().contains("240"));
    }

    #[test]
    fn turning_a_plugin_off_is_saved_and_restore_defaults_turns_it_back_on() {
        use serde_json::json;
        let mut app = crate::SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        assert!(enabled(&app, "ftc-frc-parts"));
        crate::prefs::set(&mut app, &json!({"disabled_plugins": ["ftc-frc-parts"]})).unwrap();
        assert!(!enabled(&app, "ftc-frc-parts"));
        let err = crate::prefs::set(&mut app, &json!({"disabled_plugins": ["Bad Id"]})).unwrap_err();
        assert!(err.contains("disabled_plugins"), "{err}");
        assert!(!enabled(&app, "ftc-frc-parts"), "a refused list leaves the previous choice");
        let mut other = crate::SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        other.load_prefs(&app.prefs());
        assert!(!enabled(&other, "ftc-frc-parts"));
        crate::prefs::control(&mut app, &json!({"open": "Plugins", "button": "defaults"})).unwrap();
        assert_eq!(app.prefs_window.section, crate::prefs::PAGE_PLUGINS);
        assert!(enabled(&app, "ftc-frc-parts"));
        assert!(crate::prefs::control(&mut app, &json!({"open": "NoSuch"})).is_err());
    }

    fn zip_with(files: &[(&str, &str)]) -> Vec<u8> {
        use std::io::Write;
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        for (name, body) in files {
            writer.start_file(*name, options).unwrap();
            writer.write_all(body.as_bytes()).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    const MANIFEST: &str = r#"{"id":"demo","name":"Demo parts","version":"1.0.0","description":"A small plugin."}"#;

    #[test]
    fn a_github_link_is_the_repository_and_nothing_else() {
        let repo = parse_github("https://github.com/Tangency-Labs/SolidCraft.git").unwrap();
        assert_eq!(repo.archive_url(), "https://api.github.com/repos/Tangency-Labs/SolidCraft/zipball");
        assert_eq!(parse_github("git@github.com:octocat/Hello-World.git").unwrap().caption(), "octocat/Hello-World");
        assert_eq!(parse_github("  github.com/octocat/Hello-World/tree/main  ").unwrap().git_ref.as_deref(), Some("main"));
        assert_eq!(parse_github("octocat/Hello-World/releases/tag/v1.0.0").unwrap().git_ref.as_deref(), Some("v1.0.0"));
        assert!(parse_github("https://gitlab.com/octocat/Hello-World").is_err());
        assert!(parse_github("http://github.com/octocat/Hello-World").is_err());
        assert!(parse_github("https://user:token@github.com/octocat/Hello-World").is_err());
        assert!(parse_github("https://github.com/octocat/Hello-World/blob/main/plugin.json").is_err());
        assert!(parse_github("https://github.com.evil.example/octocat/Hello-World").is_err());
    }

    #[test]
    fn search_matches_the_name_the_id_and_the_description() {
        let plugin = Plugin {
            id: "ftc-frc-parts".into(),
            name: "FTC and FRC parts".into(),
            version: "0.1.0".into(),
            description: "Commercial robot parts.".into(),
        };
        assert!(matches_query(&plugin, ""));
        assert!(matches_query(&plugin, " frc "));
        assert!(matches_query(&plugin, "FTC-FRC"));
        assert!(matches_query(&plugin, "commercial"));
        assert!(matches_query(&plugin, "0.1"));
        assert!(!matches_query(&plugin, "bearing"));
    }

    #[test]
    fn a_zip_with_one_plugin_json_is_installed_under_its_id() {
        let dir = std::env::temp_dir().join(format!("solidcraft-plugins-zip-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let bytes = zip_with(&[
            ("owner-repo-abc/README.md", "not the plugin"),
            ("owner-repo-abc/plugin/plugin.json", MANIFEST),
            ("owner-repo-abc/plugin/note.txt", "hello"),
        ]);
        let installed = install_zip(&bytes, &dir).unwrap();
        assert_eq!(installed.id, "demo");
        assert!(!installed.updated);
        assert_eq!(std::fs::read_to_string(dir.join("demo/plugin.json")).unwrap(), MANIFEST);
        assert_eq!(std::fs::read_to_string(dir.join("demo/note.txt")).unwrap(), "hello");
        assert!(!dir.join("demo/README.md").exists());
        let again = zip_with(&[("wrap/plugin.json", r#"{"id":"demo","name":"Demo parts","version":"1.0.1","description":"A small plugin."}"#)]);
        let updated = install_zip(&again, &dir).unwrap();
        assert!(updated.updated);
        assert!(std::fs::read_to_string(dir.join("demo/plugin.json")).unwrap().contains("1.0.1"));
        assert!(!dir.join("demo/note.txt").exists(), "an update replaces the folder");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn two_plugin_json_files_and_a_path_that_leaves_the_archive_are_refused() {
        let dir = std::env::temp_dir().join(format!("solidcraft-plugins-zip-bad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let two = zip_with(&[("a/plugin.json", MANIFEST), ("b/plugin.json", r#"{"id":"other","name":"Other","version":"1","description":""}"#)]);
        let err = install_zip(&two, &dir).unwrap_err();
        assert!(err.contains("2 plugin.json"), "{err}");
        assert!(!dir.join("demo").exists());
        let escaped = zip_with(&[("../escaped.txt", "no"), ("wrap/plugin.json", MANIFEST)]);
        let err = install_zip(&escaped, &dir).unwrap_err();
        assert!(err.contains("leaves its folder"), "{err}");
        assert!(!dir.join("escaped.txt").exists());
        assert!(!dir.parent().unwrap().join("escaped.txt").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
