use crate::{ImportRequest, Imported, RuntimeOptions};
#[cfg(unix)]
use cap_std::fs::OpenOptionsExt;
use cap_std::fs::{Dir, OpenOptions};
use craft_plugin::{
    CommandSchema, Effect, HostError, Limits, LossReport, MAX_EXPORT_FILE_BYTES, MAX_IMPORT_CHUNK_BYTES, MAX_IMPORT_DESIGN_BYTES,
    MAX_IMPORT_INPUT_BYTES, MAX_JOB_CALLS, MAX_JSON_BYTES, Manifest, PluginHost, Result,
};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::path::{Component as PathComponent, Path, PathBuf};
use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    mpsc,
};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use wasmtime::component::{Component, Linker};
use wasmtime::{Config, Engine, Store, StoreLimits, StoreLimitsBuilder, UpdateDeadline};

mod bindings {
    wasmtime::component::bindgen!({ path: "../craft-plugin/wit", world: "command-plugin" });
}
mod dialog_bindings {
    wasmtime::component::bindgen!({
        path: "../craft-plugin/wit", world: "dialog-plugin",
        with: {
            "craft:plugin/types": super::bindings::craft::plugin::types,
            "craft:plugin/commands": super::bindings::craft::plugin::commands,
            "craft:plugin/log": super::bindings::craft::plugin::log,
            "craft:plugin/control": super::bindings::craft::plugin::control,
            "craft:plugin/files": super::bindings::craft::plugin::files,
            "craft:plugin/clock": super::bindings::craft::plugin::clock,
        },
    });
}
include!(concat!(env!("OUT_DIR"), "/aliases.rs"));
include!(concat!(env!("OUT_DIR"), "/importer.rs"));
include!(concat!(env!("OUT_DIR"), "/importer_storage.rs"));
include!(concat!(env!("OUT_DIR"), "/dialog.rs"));
include!(concat!(env!("OUT_DIR"), "/folders.rs"));

mod folder_access;

// A separate package lets both frozen 0.1 and current 0.2 components negotiate capabilities.
mod host_bindings {
    wasmtime::component::bindgen!({ path: "../craft-plugin/wit-host", world: "host-query" });
}
use host_bindings::craft::host::capabilities;

fn link_host_query<H: PluginHost + 'static>(linker: &mut Linker<State<H>>, granted: &[String]) -> wasmtime::Result<()> {
    if granted.iter().any(|capability| capability.starts_with("host@")) {
        host_bindings::HostQuery::add_to_linker::<_, wasmtime::component::HasSelf<_>>(linker, |s| s)?;
    }
    Ok(())
}
include!(concat!(env!("OUT_DIR"), "/panels.rs"));
include!(concat!(env!("OUT_DIR"), "/exporter.rs"));

const MAX_CALLS: usize = 256;
/// An importer streams its file in chunks and reports progress as it goes.
const MAX_IMPORT_CALLS: usize = 1 << 20;
/// Files an importer may look at beside the one it imports, and names a folder listing reads.
const MAX_IMPORT_FILES: usize = 4096;
/// `O_NONBLOCK` for the platforms the runtime targets (the flag is not in `std`).
#[cfg(all(unix, any(target_os = "linux", target_os = "android")))]
const O_NONBLOCK: i32 = 0o4000;
#[cfg(all(unix, not(any(target_os = "linux", target_os = "android"))))]
const O_NONBLOCK: i32 = 0x4;
const MAX_LOG_BYTES: usize = 64 * 1024;
const EPOCH_POLL: Duration = Duration::from_millis(10);
/// The most components a path inside a granted or input folder may have.
const MAX_PATH_COMPONENTS: usize = 64;
static NEXT_FILE: AtomicU64 = AtomicU64::new(0);

/// A stalled regular-file read cannot be interrupted portably. Limit detached reads globally;
/// their permits remain occupied until the OS returns, even after the dialog stops waiting.
const MAX_PREFIX_READS: usize = 2;

/// The process-wide prefix-read counter.
#[cfg(not(test))]
fn prefix_reads() -> Arc<AtomicUsize> {
    static PREFIX_READS: OnceLock<Arc<AtomicUsize>> = OnceLock::new();
    PREFIX_READS.get_or_init(|| Arc::new(AtomicUsize::new(0))).clone()
}
/// Unit tests run in parallel, one thread each, and a dialog test's read must not be refused
/// because other tests hold the process-wide permits. Each test thread gets its own counter;
/// the limit itself is tested directly on a counter of its own.
#[cfg(test)]
fn prefix_reads() -> Arc<AtomicUsize> {
    thread_local! { static PREFIX_READS: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0)); }
    PREFIX_READS.with(Arc::clone)
}

struct PrefixPermit(Arc<AtomicUsize>);
impl Drop for PrefixPermit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

fn start_prefix_read<T: Send + 'static>(
    reads: Arc<AtomicUsize>,
    read: impl FnOnce() -> Result<T> + Send + 'static,
) -> Result<mpsc::Receiver<Result<T>>> {
    reads
        .try_update(Ordering::AcqRel, Ordering::Acquire, |n| (n < MAX_PREFIX_READS).then_some(n + 1))
        .map_err(|_| limit("outstanding dialog prefix reads"))?;
    let permit = PrefixPermit(reads);
    let (send, reply) = mpsc::channel();
    std::thread::Builder::new()
        .name("plugin-prefix".into())
        .spawn(move || {
            let result = read();
            drop(permit);
            // The receiver is dropped when its deadline expires; late bytes have no consumer.
            let _ = send.send(result);
        })
        .map_err(|e| trap(format!("could not start prefix reader: {e}")))?;
    Ok(reply)
}

fn wait_prefix<T>(reply: &mpsc::Receiver<Result<T>>, end: Instant) -> Result<T> {
    let result = reply.recv_timeout(end.saturating_duration_since(Instant::now()));
    if Instant::now() >= end {
        return Err(deadline());
    }
    match result {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => Err(deadline()),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(trap("prefix reader stopped unexpectedly")),
    }
}

fn limit(name: &str) -> HostError {
    HostError::LimitExceeded { limit: name.into() }
}
/// The deadline passed. [`with_limits`] fills in the enforced deadline.
fn deadline() -> HostError {
    HostError::Exhausted { limit: "deadline_ms".into(), allowed: 0 }
}
/// Fills in the budget a [`deadline`] or other placeholder left out, once the call is over.
fn with_limits(error: HostError, deadline_ms: u64) -> HostError {
    match error {
        HostError::Exhausted { limit, allowed: 0 } if limit == "deadline_ms" => HostError::Exhausted { limit, allowed: deadline_ms },
        error => error,
    }
}
fn trap(reason: impl Into<String>) -> HostError {
    HostError::Trap { reason: reason.into() }
}
fn wit_error(error: HostError) -> PluginError {
    match error {
        error @ HostError::StaleRevision { .. } => {
            PluginError::CommandFailed(types::CommandFailure { command: "plugin.run".into(), reason: error.to_string() })
        }
        HostError::PermissionDenied { permission } => PluginError::PermissionDenied(permission),
        HostError::CommandFailed { command, reason } => PluginError::CommandFailed(types::CommandFailure { command, reason }),
        HostError::LimitExceeded { limit } => PluginError::LimitExceeded(limit),
        HostError::InvalidManifest { field, reason } => PluginError::InvalidManifest(types::FieldFailure { field, reason }),
        HostError::InvalidInput { field, reason } => PluginError::InvalidInput(types::FieldFailure { field, reason }),
        // Commit conflicts normally return directly to the caller after guest execution. Keep
        // the frozen WIT error set when a host also reports one during a guest import.
        error @ HostError::UserStorageConflict { .. } => {
            PluginError::CommandFailed(types::CommandFailure { command: "user-storage.commit".into(), reason: error.to_string() })
        }
        HostError::Trap { reason } => PluginError::Trap(reason),
        HostError::Cancelled => PluginError::Cancelled,
        HostError::UnsupportedCapability { reason, .. } => PluginError::InvalidManifest(types::FieldFailure { field: "capabilities".into(), reason }),
        HostError::Unsupported { reason } => PluginError::Unsupported(reason),
        error @ HostError::InteractiveRequired { .. } => PluginError::Unsupported(error.to_string()),
        error @ (HostError::OverCeiling { .. } | HostError::Exhausted { .. }) => PluginError::LimitExceeded(error.to_string()),
    }
}
fn host_error(error: PluginError) -> HostError {
    match error {
        PluginError::PermissionDenied(permission) => HostError::PermissionDenied { permission },
        PluginError::CommandFailed(types::CommandFailure { command, reason }) => HostError::CommandFailed { command, reason },
        PluginError::LimitExceeded(limit) => HostError::LimitExceeded { limit },
        PluginError::InvalidManifest(types::FieldFailure { field, reason }) => HostError::InvalidManifest { field, reason },
        PluginError::InvalidInput(types::FieldFailure { field, reason }) => HostError::InvalidInput { field, reason },
        PluginError::Trap(reason) => trap(reason),
        PluginError::Cancelled => HostError::Cancelled,
        PluginError::Unsupported(reason) => HostError::Unsupported { reason },
    }
}
struct Folder {
    dir: Dir,
    path: PathBuf,
    read: bool,
    write: bool,
}
struct PendingWrite {
    temporary: PathBuf,
    order: usize,
}
struct State<H> {
    host: H,
    commands: BTreeMap<String, CommandSchema>,
    /// Sensitive command groups both the manifest requests and the app grants.
    groups: BTreeSet<String>,
    granted_capabilities: Vec<String>,
    effect: Effect,
    user_storage: bool,
    design_tokens: bool,
    folders: BTreeMap<String, Folder>,
    input_folders: BTreeMap<String, Folder>,
    folder_bytes: u64,
    folder_files: BTreeSet<(String, PathBuf)>,
    staged: BTreeMap<(String, PathBuf), PendingWrite>,
    /// Folders `write-file` made inside a grant for a staged file, in the order they were made.
    /// A run that does not publish removes the ones still empty.
    created_dirs: Vec<(String, PathBuf)>,
    clock: bool,
    forms: bool,
    limits: StoreLimits,
    failed: Option<HostError>,
    calls: usize,
    max_calls: usize,
    log_bytes: usize,
    deadline: Instant,
    /// The file an importer reads and the design it writes; `None` for a command.
    import: Option<ImportState>,
    panels: Option<craft_plugin::ui::Declarations>,
}
struct ImportState {
    /// The folder of the file being imported, opened by the app's authority.
    dir: Dir,
    /// Prefix snapshot and full size during a read-only dialog hook.
    dialog: Option<(Vec<u8>, u64)>,
    name: String,
    input_folder: bool,
    read_bytes: u64,
    files: BTreeSet<PathBuf>,
    design: Vec<u8>,
    /// The manifest asks for `evaluate@0.1`.
    evaluate: bool,
    /// The design staged for `evaluate`, and whether the host has it open.
    scratch: Vec<u8>,
    evaluating: bool,
}
impl<H: PluginHost> State<H> {
    fn fail(&mut self, error: HostError) -> PluginError {
        self.failed.get_or_insert_with(|| error.clone());
        wit_error(error)
    }
    fn check(&mut self) -> std::result::Result<(), PluginError> {
        if let Some(error) = &self.failed {
            return Err(wit_error(error.clone()));
        }
        if self.host.is_cancelled() {
            return Err(self.fail(HostError::Cancelled));
        }
        if Instant::now() >= self.deadline {
            return Err(self.fail(deadline()));
        }
        self.calls = self.calls.saturating_add(1);
        if self.calls > self.max_calls {
            return Err(self.fail(limit("host call count")));
        }
        Ok(())
    }
    fn file_path(&mut self, grant: &str, path: &str, write: bool) -> std::result::Result<(), PluginError> {
        self.check()?;
        let valid = !path.is_empty() && Path::new(path).components().all(|c| matches!(c, PathComponent::Normal(_)));
        let granted = self.folders.get(grant).is_some_and(|f| if write { f.write } else { f.read });
        if !valid || !granted {
            return Err(self.fail(HostError::PermissionDenied { permission: format!("files.{}:{grant}", if write { "write" } else { "read" }) }));
        }
        // The same limit `folders.read` puts on a path inside a folder.
        if path.len() > 4096 || Path::new(path).components().count() > MAX_PATH_COMPONENTS {
            return Err(self.fail(HostError::InvalidInput {
                field: "path".into(),
                reason: format!("files.{}:{grant}: a path may have at most {MAX_PATH_COMPONENTS} components", if write { "write" } else { "read" }),
            }));
        }
        if let Some(folder) = self.folders.get(grant) {
            if let Err(error) = refuse_links(&folder.dir, Path::new(path)) {
                let permission = format!("files.{}:{grant}: {error}", if write { "write" } else { "read" });
                return Err(self.fail(HostError::PermissionDenied { permission }));
            }
            self.host.authorize_file_path(&folder.path.join(path), write).map_err(|error| self.fail(error))?;
        }
        Ok(())
    }
    fn discard_writes(&mut self) {
        for ((grant, _), pending) in std::mem::take(&mut self.staged) {
            if let Some(folder) = self.folders.get(&grant) {
                let _ = folder.dir.remove_file(pending.temporary);
            }
        }
        // Newest first, so a folder's sub-folders go before it; a folder that now holds anything stays.
        for (grant, dir) in std::mem::take(&mut self.created_dirs).into_iter().rev() {
            if let Some(folder) = self.folders.get(&grant) {
                let _ = folder.dir.remove_dir(dir);
            }
        }
    }
    fn publish_writes(&mut self) -> Result<()> {
        let mut writes: Vec<_> = self.staged.iter().collect();
        writes.sort_by_key(|(_, pending)| pending.order);
        for ((grant, path), pending) in writes {
            let folder = self.folders.get(grant).ok_or_else(|| trap("file grant disappeared before publication"))?;
            folder.dir.rename(&pending.temporary, &folder.dir, path).map_err(|error| HostError::PermissionDenied {
                permission: format!("files.write:{grant}: publication failed after app commit; files may have changed: {error}"),
            })?;
        }
        // The published files now live in the folders made for them.
        self.created_dirs.clear();
        Ok(())
    }
}
impl<H: PluginHost> capabilities::Host for State<H> {
    fn granted(&mut self) -> Vec<String> {
        if self.check().is_err() {
            return Vec::new();
        }
        self.granted_capabilities.clone()
    }
}
impl<H: PluginHost> types::Host for State<H> {}
impl<H: PluginHost> commands::Host for State<H> {
    fn run(&mut self, command: String, args: String) -> std::result::Result<String, PluginError> {
        self.check()?;
        if args.len() > MAX_JSON_BYTES {
            return Err(self.fail(limit("command JSON size")));
        }
        let Some(schema) = self.commands.get(&command) else {
            return Err(self.fail(HostError::PermissionDenied { permission: format!("command {command} is not granted") }));
        };
        if let Some(group) = schema.group.as_deref()
            && !self.groups.contains(group)
        {
            let permission = format!("command {command} needs the `{group}` permission (runtime.permissions.command_groups), which is not granted");
            return Err(self.fail(HostError::PermissionDenied { permission }));
        }
        if self.effect == Effect::Read && schema.effect != Effect::Read {
            return Err(self.fail(HostError::PermissionDenied { permission: format!("edit command {command} from read entry") }));
        }
        let args = serde_json::from_str(&args).map_err(|e| self.fail(HostError::InvalidInput { field: command.clone(), reason: e.to_string() }))?;
        let result = self.host.run_command(&command, &args).map_err(|e| self.fail(e))?;
        let text = serde_json::to_string(&result).map_err(|e| self.fail(trap(e.to_string())))?;
        if text.len() > MAX_JSON_BYTES {
            return Err(self.fail(limit("command result size")));
        }
        Ok(text)
    }
}
impl<H: PluginHost> State<H> {
    fn user_storage_check(&mut self, key: Option<&str>, write: Option<&str>) -> std::result::Result<(), PluginError> {
        self.check()?;
        if !self.user_storage {
            return Err(self.fail(HostError::PermissionDenied { permission: "user-storage@0.1 is not requested".into() }));
        }
        if let Some(import) = write.filter(|_| self.effect == Effect::Read) {
            return Err(self.fail(craft_plugin::read_entry_refusal(import)));
        }
        if let Some(key) = key {
            craft_plugin::check_user_storage_key(key).map_err(|e| self.fail(e))?;
        }
        Ok(())
    }
}
impl<H: PluginHost> user_storage::Host for State<H> {
    fn get(&mut self, key: String) -> std::result::Result<Option<String>, PluginError> {
        self.user_storage_check(Some(&key), None)?;
        let value = self.host.user_storage_get(&key).map_err(|e| self.fail(e))?;
        let json = value.map(|v| serde_json::to_string(&v)).transpose().map_err(|e| self.fail(trap(e.to_string())))?;
        if json.as_ref().is_some_and(|v| v.len() > craft_plugin::MAX_USER_STORAGE_BYTES) {
            return Err(self.fail(limit("user-storage total bytes (65536)")));
        }
        Ok(json)
    }
    fn set(&mut self, key: String, value: String) -> std::result::Result<(), PluginError> {
        self.user_storage_check(Some(&key), Some("user-storage.set"))?;
        if value.len() > MAX_JSON_BYTES {
            return Err(self.fail(limit("user-storage JSON input bytes (1048576)")));
        }
        let value = serde_json::from_str(&value)
            .map_err(|e| self.fail(HostError::InvalidInput { field: "user-storage.value".into(), reason: e.to_string() }))?;
        self.host.user_storage_set(&key, &value).map_err(|e| self.fail(e))
    }
    fn remove(&mut self, key: String) -> std::result::Result<bool, PluginError> {
        self.user_storage_check(Some(&key), Some("user-storage.remove"))?;
        self.host.user_storage_remove(&key).map_err(|e| self.fail(e))
    }
    fn list(&mut self) -> std::result::Result<Vec<String>, PluginError> {
        self.user_storage_check(None, None)?;
        let keys = self.host.user_storage_list().map_err(|e| self.fail(e))?;
        if keys.len() > craft_plugin::MAX_USER_STORAGE_KEYS {
            return Err(self.fail(limit("user-storage keys (128)")));
        }
        for key in &keys {
            craft_plugin::check_user_storage_key(key).map_err(|e| self.fail(e))?;
        }
        Ok(keys)
    }
}
impl<H: PluginHost> design_state::Host for State<H> {
    fn get_token(&mut self) -> std::result::Result<String, PluginError> {
        self.check()?;
        if !self.design_tokens {
            return Err(self.fail(HostError::PermissionDenied { permission: "design@0.1 capability is required".into() }));
        }
        let token = self.host.design_token().map_err(|error| self.fail(error))?;
        let text = serde_json::to_string(&token).map_err(|error| self.fail(trap(error.to_string())))?;
        if text.len() > MAX_JSON_BYTES {
            return Err(self.fail(limit("design token JSON size")));
        }
        Ok(text)
    }
}
impl<H: PluginHost> log::Host for State<H> {
    fn log(&mut self, level: log::Level, message: String) {
        if self.check().is_err() {
            return;
        }
        self.log_bytes = self.log_bytes.saturating_add(message.len());
        if message.len() > 1024 || self.log_bytes > MAX_LOG_BYTES {
            self.fail(limit("log size"));
            return;
        }
        let clean: String = message.chars().filter(|c| !c.is_control()).collect();
        match level {
            log::Level::Debug => ::log::debug!(target: "craft_plugin_wasm", "{clean}"),
            log::Level::Info => ::log::info!(target: "craft_plugin_wasm", "{clean}"),
            log::Level::Warn => ::log::warn!(target: "craft_plugin_wasm", "{clean}"),
            log::Level::Error => ::log::error!(target: "craft_plugin_wasm", "{clean}"),
        }
        let host_level = match level {
            log::Level::Debug => craft_plugin::LogLevel::Debug,
            log::Level::Info => craft_plugin::LogLevel::Info,
            log::Level::Warn => craft_plugin::LogLevel::Warn,
            log::Level::Error => craft_plugin::LogLevel::Error,
        };
        self.host.log_level(host_level, &clean);
    }
}
impl<H: PluginHost> control::Host for State<H> {
    fn progress(&mut self, fraction: f32, message: String) {
        if self.check().is_err() {
            return;
        }
        if !fraction.is_finite() || !(0.0..=1.0).contains(&fraction) || message.len() > 1024 {
            self.fail(HostError::InvalidInput {
                field: "progress".into(),
                reason: "fraction must be finite and in 0..1; message at most 1024 bytes".into(),
            });
            return;
        }
        self.host.progress(fraction, &message.chars().filter(|c| !c.is_control()).collect::<String>());
    }
    fn cancelled(&mut self) -> bool {
        let cancelled = self.host.is_cancelled();
        if cancelled {
            self.fail(HostError::Cancelled);
        }
        cancelled
    }
}
impl<H: PluginHost> forms::Host for State<H> {
    fn open(&mut self, entry: String, params: String) -> std::result::Result<(), PluginError> {
        self.check()?;
        if !self.forms {
            return Err(self.fail(HostError::PermissionDenied { permission: "forms@0.1 is not declared".into() }));
        }
        if !craft_plugin::valid_id(&entry) || params.len() > MAX_JSON_BYTES {
            return Err(self.fail(HostError::InvalidInput { field: "form".into(), reason: "invalid entry or parameters exceed 1 MiB".into() }));
        }
        let value: Value =
            serde_json::from_str(&params).map_err(|e| self.fail(HostError::InvalidInput { field: "params".into(), reason: e.to_string() }))?;
        self.host.open_form(&entry, &value).map_err(|e| self.fail(e))
    }
}

impl<H: PluginHost> clock::Host for State<H> {
    fn now(&mut self) -> u64 {
        if !self.clock || self.check().is_err() {
            return 0;
        }
        SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or_default()
    }
}
impl<H: PluginHost> storage::Host for State<H> {
    fn put_blob(&mut self, name: String, data: Vec<u8>, cache: bool, append: bool) -> std::result::Result<u64, PluginError> {
        // A read entry stays read-only in every way: no change to the plugin's data either.
        if self.effect == Effect::Read {
            self.check()?;
            return Err(self.fail(craft_plugin::read_entry_refusal("storage.put-blob")));
        }
        self.check()?;
        if data.len() > MAX_JSON_BYTES {
            return Err(self.fail(limit("storage write size")));
        }
        self.host.storage_put_blob(&name, &data, cache, append).map_err(|error| self.storage_error(error))
    }
    fn get_blob(&mut self, plugin: Option<String>, name: String, offset: u64, len: u32) -> std::result::Result<Vec<u8>, PluginError> {
        self.check()?;
        if len as usize > MAX_JSON_BYTES {
            return Err(self.fail(limit("storage read size")));
        }
        self.host.storage_get_blob(plugin.as_deref(), &name, offset, len).map_err(|error| self.storage_error(error))
    }
    fn get_document(&mut self, plugin: Option<String>, offset: u64, len: u32) -> std::result::Result<Vec<u8>, PluginError> {
        self.check()?;
        if len as usize > MAX_JSON_BYTES {
            return Err(self.fail(limit("storage read size")));
        }
        self.host.storage_get_document(plugin.as_deref(), offset, len).map_err(|error| self.storage_error(error))
    }
}
impl<H: PluginHost> State<H> {
    /// A storage call's error: a file that isn't there or an argument out of range is an answer the
    /// plugin can act on; anything else (a permission, a limit) fails the run.
    fn storage_error(&mut self, error: HostError) -> PluginError {
        match error {
            HostError::InvalidInput { .. } => wit_error(error),
            error => self.fail(error),
        }
    }
}
impl<H: PluginHost> files::Host for State<H> {
    fn read_file(&mut self, grant: String, path: String) -> std::result::Result<Vec<u8>, PluginError> {
        self.file_path(&grant, &path, false)?;
        // `None`: the path is allowed and nothing is there (and no write of this call is staged for it).
        let read = (|| -> std::io::Result<Option<Vec<u8>>> {
            let folder = self.folders.get(&grant).ok_or_else(|| std::io::Error::other("grant disappeared"))?;
            let staged = self.staged.get(&(grant.clone(), Path::new(&path).components().collect()));
            let file = match folder.dir.open(staged.map(|pending| pending.temporary.as_path()).unwrap_or(Path::new(&path))) {
                Err(error) if staged.is_none() && error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                opened => opened?,
            };
            if !file.metadata()?.is_file() {
                return Err(std::io::Error::other("only regular files are supported"));
            }
            let mut bytes = Vec::new();
            file.take(MAX_JSON_BYTES as u64 + 1).read_to_end(&mut bytes)?;
            Ok(Some(bytes))
        })();
        let bytes = read.map_err(|e| self.fail(HostError::PermissionDenied { permission: format!("files.read:{grant}: {e}") }))?;
        // A file that is not there yet is an answer, not a failed run (#17899): the entry gets the
        // error and goes on, so a plugin can make its own file on its first run.
        let Some(bytes) = bytes else {
            return Err(wit_error(HostError::InvalidInput { field: "path".into(), reason: "no such file".into() }));
        };
        if bytes.len() > MAX_JSON_BYTES {
            return Err(self.fail(limit("file read size")));
        }
        Ok(bytes)
    }
    fn write_file(&mut self, grant: String, path: String, data: Vec<u8>) -> std::result::Result<(), PluginError> {
        // A read entry stays read-only in every way: no file write, whatever folders are granted.
        if self.effect == Effect::Read {
            self.check()?;
            return Err(self.fail(craft_plugin::read_entry_refusal("write-file")));
        }
        self.file_path(&grant, &path, true)?;
        let path: PathBuf = Path::new(&path).components().collect();
        if data.len() > MAX_JSON_BYTES {
            return Err(self.fail(limit("file write size")));
        }
        let mut created = Vec::new();
        let write = (|| -> std::io::Result<PathBuf> {
            let folder = self.folders.get(&grant).ok_or_else(|| std::io::Error::other("grant disappeared"))?;
            let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
            // A file in a sub-folder that does not exist yet gets that folder, inside the grant
            // (#20493). Each step was checked above: plain names, no links, the whole path allowed.
            let mut prefix = PathBuf::new();
            for component in parent.components().filter(|c| matches!(c, PathComponent::Normal(_))) {
                prefix.push(component);
                match folder.dir.symlink_metadata(&prefix) {
                    Ok(metadata) if metadata.is_dir() => {}
                    Ok(_) => return Err(std::io::Error::other(format!("{} is a file, not a folder", prefix.display()))),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        folder.dir.create_dir(&prefix)?;
                        created.push(prefix.clone());
                    }
                    Err(error) => return Err(error),
                }
            }
            let temp = parent.join(format!(".craft-plugin-{}-{}", std::process::id(), NEXT_FILE.fetch_add(1, Ordering::Relaxed)));
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            let mut file = folder.dir.open_with(&temp, &options)?;
            let result = (|| {
                file.write_all(&data)?;
                file.sync_all()?;
                Ok(())
            })();
            if result.is_err() {
                let _ = folder.dir.remove_file(&temp);
            }
            result.map(|()| temp)
        })();
        self.created_dirs.extend(created.into_iter().map(|dir| (grant.clone(), dir)));
        // Only a refusal by the system is a denied permission; anything else is about this path.
        let temporary = write.map_err(|e| {
            self.fail(if e.kind() == std::io::ErrorKind::PermissionDenied {
                HostError::PermissionDenied { permission: format!("files.write:{grant}: {e}") }
            } else {
                HostError::InvalidInput { field: "path".into(), reason: format!("files.write:{grant}: {e}") }
            })
        })?;
        if let Some(previous) = self.staged.insert((grant.clone(), path), PendingWrite { temporary, order: self.calls })
            && let Some(folder) = self.folders.get(&grant)
        {
            let _ = folder.dir.remove_file(previous.temporary);
        }
        Ok(())
    }
}

impl<H: PluginHost> State<H> {
    /// The file `path` names for an importer: "" is the file being imported; anything else is a
    /// plain relative path in its folder, with no hidden names or links, and needs `input_folder`.
    fn source_path(&mut self, path: &str) -> std::result::Result<PathBuf, PluginError> {
        self.check()?;
        let Some(import) = self.import.as_ref() else {
            return Err(self.fail(HostError::PermissionDenied { permission: "source: this call is not an import".into() }));
        };
        if path.is_empty() {
            return Ok(PathBuf::from(&import.name));
        }
        if !import.input_folder {
            let permission = format!("input_folder: reading {path} beside the imported file needs the input_folder permission");
            return Err(self.fail(HostError::PermissionDenied { permission }));
        }
        let relative: PathBuf = Path::new(path).components().collect();
        let plain = path.len() <= 4096
            && !relative.as_os_str().is_empty()
            && relative.components().count() <= MAX_PATH_COMPONENTS
            && relative.components().all(|c| matches!(c, PathComponent::Normal(name) if !name.to_string_lossy().starts_with('.')));
        if !plain {
            return Err(self.fail(HostError::PermissionDenied { permission: format!("input_folder: {path} is not a plain path inside the folder") }));
        }
        if let Err(error) = refuse_links(&import.dir, &relative) {
            return Err(self.fail(HostError::PermissionDenied { permission: format!("input_folder: {error}") }));
        }
        Ok(relative)
    }
    fn count_file(&mut self, path: &Path) -> std::result::Result<(), PluginError> {
        let Some(import) = self.import.as_mut() else { return Ok(()) };
        if import.files.len().saturating_add(self.folder_files.len()) >= MAX_IMPORT_FILES && !import.files.contains(path) {
            return Err(self.fail(limit("input_folder: more than 4096 files")));
        }
        import.files.insert(path.to_path_buf());
        Ok(())
    }
}
impl<H: PluginHost> source::Host for State<H> {
    fn read(&mut self, path: String, offset: u64, len: u32) -> std::result::Result<Vec<u8>, PluginError> {
        let target = self.source_path(&path)?;
        let len = len as usize;
        if len == 0 || len > MAX_IMPORT_CHUNK_BYTES {
            return Err(self.fail(limit("source.read chunk (1 byte to 1 MiB)")));
        }
        if let Some(import) = self.import.as_mut()
            && let Some((prefix, _)) = &import.dialog
        {
            let cap = craft_plugin::dialog::MAX_PREFIX_BYTES;
            if offset.checked_add(len as u64).is_none_or(|end| end > cap) || import.read_bytes.saturating_add(len as u64) > cap {
                return Err(self.fail(limit("dialog source: only 64 KiB of prefix reads per update")));
            }
            import.read_bytes += len as u64;
            let start = (offset as usize).min(prefix.len());
            let end = start.saturating_add(len).min(prefix.len());
            return Ok(prefix.get(start..end).unwrap_or_default().to_vec());
        }
        self.count_file(&target)?;
        let read = (|| -> std::io::Result<Vec<u8>> {
            use std::io::{Seek, SeekFrom};
            let import = self.import.as_ref().ok_or_else(|| std::io::Error::other("not an import"))?;
            // Non-blocking, so a named pipe in the folder cannot hold the import in `open`; the
            // opened handle is then checked, not the path.
            let mut options = OpenOptions::new();
            options.read(true);
            #[cfg(unix)]
            options.custom_flags(O_NONBLOCK);
            let mut file = import.dir.open_with(&target, &options)?;
            if !file.metadata()?.is_file() {
                return Err(std::io::Error::other("only regular files are read"));
            }
            file.seek(SeekFrom::Start(offset))?;
            let mut bytes = Vec::with_capacity(len);
            file.take(len as u64).read_to_end(&mut bytes)?;
            Ok(bytes)
        })();
        let bytes = read.map_err(|e| self.fail(HostError::PermissionDenied { permission: format!("source.read {path}: {e}") }))?;
        if let Some(import) = self.import.as_mut() {
            import.read_bytes = import.read_bytes.saturating_add(bytes.len() as u64);
            if import.read_bytes.saturating_add(self.folder_bytes) > MAX_IMPORT_INPUT_BYTES {
                return Err(self.fail(limit("source: more than 1 GiB read in one import")));
            }
        }
        Ok(bytes)
    }
    fn size(&mut self, path: String) -> std::result::Result<Option<u64>, PluginError> {
        let target = self.source_path(&path)?;
        let import = self.import.as_ref().ok_or_else(|| PluginError::Trap("not an import".into()))?;
        if let Some((_, size)) = &import.dialog {
            return Ok(Some(*size));
        }
        match import.dir.metadata(&target) {
            Ok(metadata) if metadata.is_file() => Ok(Some(metadata.len())),
            Ok(_) => Ok(None),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(self.fail(HostError::PermissionDenied { permission: format!("source.size {path}: {error}") })),
        }
    }
    fn resolve(&mut self, path: String) -> std::result::Result<Option<String>, PluginError> {
        let target = self.source_path(&path)?;
        if path.is_empty() {
            return Ok(Some(String::new()));
        }
        let import = self.import.as_ref().ok_or_else(|| PluginError::Trap("not an import".into()))?;
        if import.dir.metadata(&target).is_ok_and(|m| m.is_file()) {
            return Ok(Some(target.to_string_lossy().replace('\\', "/")));
        }
        // The one file in that folder whose name matches ignoring case: a part an assembly
        // spelled differently from the disk (made on a case-insensitive system).
        let (Some(parent), Some(wanted)) = (target.parent(), target.file_name().map(|n| n.to_string_lossy().to_lowercase())) else {
            return Ok(None);
        };
        let listing = if parent.as_os_str().is_empty() { import.dir.entries() } else { import.dir.read_dir(parent) };
        let Ok(listing) = listing else { return Ok(None) };
        let mut found = None;
        let mut scanned = 0;
        for entry in listing.flatten() {
            scanned += 1;
            if scanned > MAX_IMPORT_FILES {
                // A partial scan cannot show the match is the only one.
                return Ok(None);
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || name.to_lowercase() != wanted || !entry.file_type().is_ok_and(|t| t.is_file()) {
                continue;
            }
            if found.is_some() {
                return Ok(None);
            }
            found = Some(parent.join(name).to_string_lossy().replace('\\', "/"));
        }
        Ok(found)
    }
}
impl<H: PluginHost> design::Host for State<H> {
    fn write(&mut self, bytes: Vec<u8>) -> std::result::Result<(), PluginError> {
        self.check()?;
        if self.import.as_ref().is_some_and(|import| import.dialog.is_some()) {
            return Err(self.fail(HostError::PermissionDenied { permission: "dialog hooks cannot write designs".into() }));
        }
        if bytes.len() > MAX_IMPORT_CHUNK_BYTES {
            return Err(self.fail(limit("design.write chunk (at most 1 MiB)")));
        }
        let Some(import) = self.import.as_mut() else {
            return Err(self.fail(HostError::PermissionDenied { permission: "design: this call is not an import".into() }));
        };
        if import.design.len().saturating_add(bytes.len()) > MAX_IMPORT_DESIGN_BYTES {
            let allowed = (MAX_IMPORT_DESIGN_BYTES >> 20) as u64;
            return Err(self.fail(HostError::Exhausted { limit: "design_mib".into(), allowed }));
        }
        import.design.extend_from_slice(&bytes);
        Ok(())
    }
}

impl<H: PluginHost> State<H> {
    /// The import, when its manifest asks for `evaluate`.
    fn evaluation(&mut self) -> std::result::Result<&mut ImportState, PluginError> {
        self.check()?;
        match self.import.as_mut() {
            Some(import) if import.evaluate => Ok(import),
            _ => Err(wit_error(HostError::PermissionDenied { permission: "evaluate: the manifest does not ask for evaluate@0.1".into() })),
        }
    }
    fn close_evaluation(&mut self) {
        if let Some(import) = self.import.as_mut()
            && std::mem::take(&mut import.evaluating)
        {
            self.host.evaluate_close();
        }
    }
}
impl<H: PluginHost> evaluate::Host for State<H> {
    fn stage(&mut self, bytes: Vec<u8>) -> std::result::Result<(), PluginError> {
        if bytes.len() > MAX_IMPORT_CHUNK_BYTES {
            self.check()?;
            return Err(self.fail(limit("evaluate.stage chunk (at most 1 MiB)")));
        }
        if self.evaluation()?.evaluating {
            self.close_evaluation();
            if let Some(import) = self.import.as_mut() {
                import.scratch.clear();
            }
        }
        let import = self.evaluation()?;
        if import.scratch.len().saturating_add(bytes.len()) > MAX_IMPORT_DESIGN_BYTES {
            return Err(self.fail(limit("evaluate: a staged design of more than 256 MiB")));
        }
        import.scratch.extend_from_slice(&bytes);
        Ok(())
    }
    fn run(&mut self, command: String, args: String) -> std::result::Result<String, PluginError> {
        if args.len() > MAX_JSON_BYTES || command.len() > 128 {
            self.check()?;
            return Err(self.fail(limit("evaluate.run arguments exceed 1 MiB")));
        }
        let args: Value =
            serde_json::from_str(&args).map_err(|e| wit_error(HostError::InvalidInput { field: "args".into(), reason: e.to_string() }))?;
        if !self.evaluation()?.evaluating {
            let design = std::mem::take(&mut self.evaluation()?.scratch);
            // The plugin's own design not opening is an answer about it, not a failed import.
            self.host.evaluate_open(&design).map_err(wit_error)?;
            self.evaluation()?.evaluating = true;
        }
        let result = self.host.evaluate_run(&command, &args).map_err(wit_error)?;
        let text = serde_json::to_string(&result).map_err(|e| wit_error(trap(e.to_string())))?;
        if text.len() > MAX_JSON_BYTES {
            // Too large to check this way: an answer, not a failed import.
            return Err(wit_error(limit("evaluate.run result exceeds 1 MiB")));
        }
        Ok(text)
    }
    fn reset(&mut self) {
        self.close_evaluation();
        if let Some(import) = self.import.as_mut() {
            import.scratch = Vec::new();
        }
    }
}

/// Refuses `path` inside a granted folder when any part of it is a symbolic link (or, on Windows,
/// a junction). Authorization looks at the path as spelled, so a link would let a visible name
/// reach what the spelling hides: `public -> .hidden` made `public/secret.txt` readable and
/// writable. Parts that do not exist yet (a new file) are fine. The guest cannot create links (it
/// only writes new regular files), so the gap between this check and the open is reachable only
/// by another program that can already write the granted folder.
fn refuse_links(dir: &Dir, path: &Path) -> std::io::Result<()> {
    let mut prefix = PathBuf::new();
    for component in path.components() {
        prefix.push(component);
        match dir.symlink_metadata(&prefix) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(std::io::Error::other(format!("{} is a link; links are not followed", prefix.display())));
            }
            // Nothing lies past a plain file, so no link can either: the caller reports a file
            // where a folder should be as a wrong path, not as a refused permission (#20493).
            Ok(metadata) if !metadata.is_dir() => return Ok(()),
            Ok(_) => {}
            Err(error) if matches!(error.kind(), std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory) => return Ok(()),
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn execution_error(failure: &wasmtime::Error, remaining: Option<u64>, limits: &Limits, stack_bytes: usize) -> HostError {
    let trap_name = failure.downcast_ref::<wasmtime::Trap>().map(|t| format!("{t:?}")).unwrap_or_else(|| "none".into());
    let fuel = remaining.map(|n| format!("fuel_consumed={}, fuel_remaining={n}", limits.fuel.saturating_sub(n))).unwrap_or_default();
    let reason = format!(
        "component execution failed: {} (trap={trap_name}, {fuel}, fuel_limit={}, memory_limit_mib={}, deadline_ms={}): {failure:#}",
        failure.root_cause(),
        limits.fuel,
        limits.memory_mib,
        limits.deadline_ms
    );
    // A budget the plugin ran into is said plainly; the engine's details are for its author.
    let exhausted = |limit: &str, allowed: u64| {
        ::log::debug!(target: "craft_plugin_wasm", "plugin stopped: {reason}");
        HostError::Exhausted { limit: limit.into(), allowed }
    };
    match failure.downcast_ref::<wasmtime::Trap>() {
        Some(wasmtime::Trap::OutOfFuel) => exhausted("fuel", limits.fuel),
        Some(wasmtime::Trap::StackOverflow) => exhausted("stack_kib", (stack_bytes / 1024) as u64),
        _ if reason.contains("growing memory") => exhausted("memory_mib", limits.memory_mib as u64),
        _ => trap(reason),
    }
}

/// The WIT a component was built against: the current [`craft_plugin::API`] or the frozen 0.1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Wit {
    Current,
    V0_1,
}

/// Record an interface version before compilation can fail on a validator limit.
fn note_version(name: &str, found: &mut BTreeSet<String>) -> Result<()> {
    let package = craft_plugin::API.split_once('@').map(|(package, _)| package).unwrap_or(craft_plugin::API);
    let version_of = |name: &str| -> Option<String> {
        name.strip_prefix(&format!("{package}/")).and_then(|rest| rest.split_once('@')).map(|(_, version)| version.chars().take(32).collect())
    };
    let Some(version) = version_of(name) else { return Ok(()) };
    if !craft_plugin::WIT_VERSIONS.contains(&version.as_str()) {
        return Err(HostError::UnsupportedCapability {
            capability: format!("{package}@{version}"),
            reason: format!(
                "built against {package}@{version}, and this host runs {package} {}; update the app or the plugin",
                craft_plugin::WIT_VERSIONS.join(" and ")
            ),
        });
    }
    found.insert(version);
    Ok(())
}

/// Refuse reserved network imports even when Wasmtime elides an unused import instance, and read
/// the WIT version the component's imports and exports name: one the host runs, and only one.
fn check_imports(engine: &Engine, component: &Component) -> Result<Wit> {
    use wasmtime::component::types::ComponentItem;
    let package = craft_plugin::API.split_once('@').map(|(package, _)| package).unwrap_or(craft_plugin::API);
    let mut found = BTreeSet::new();
    /// One import (`import`) or export and everything in it, at most `remaining` items in all.
    #[allow(clippy::too_many_arguments)]
    fn check(
        engine: &Engine,
        package: &str,
        import: bool,
        name: &str,
        item: ComponentItem,
        depth: usize,
        remaining: &mut usize,
        note: &mut dyn FnMut(&str) -> Result<()>,
    ) -> Result<()> {
        if depth > 64 || *remaining == 0 {
            return Err(limit(if import { "plugin import interface count or depth" } else { "plugin export interface count or depth" }));
        }
        *remaining -= 1;
        note(name)?;
        // Only an import reaches the network; an export named like one is the plugin's own.
        if import
            && (name == "fetch"
                || name.starts_with("wasi:sockets/")
                || name.starts_with("wasi:http/")
                || name.starts_with(&format!("{package}/fetch")))
        {
            return Err(HostError::PermissionDenied { permission: format!("network import is forbidden: {name}") });
        }
        if let ComponentItem::ComponentInstance(instance) = item {
            for (name, export) in instance.exports(engine) {
                check(engine, package, import, name, export.ty, depth + 1, remaining, note)?;
            }
        }
        Ok(())
    }
    let mut remaining = 4096;
    for (name, import) in component.component_type().imports(engine) {
        check(engine, package, true, name, import.ty, 0, &mut remaining, &mut |name| note_version(name, &mut found))?;
    }
    // Exports are read the same way, members of exported instances too, with a budget of their own.
    let mut remaining = 4096;
    for (name, export) in component.component_type().exports(engine) {
        check(engine, package, false, name, export.ty, 0, &mut remaining, &mut |name| note_version(name, &mut found))?;
    }
    if found.len() > 1 {
        let versions = found.iter().map(String::as_str).collect::<Vec<_>>().join(" and ");
        return Err(HostError::UnsupportedCapability {
            capability: package.into(),
            reason: format!("mixes {package} {versions}; build the plugin against one version"),
        });
    }
    Ok(if found.first().is_some_and(|version| version == "0.1.0") { Wit::V0_1 } else { Wit::Current })
}

pub(super) fn run<H: PluginHost + 'static>(
    host: H,
    manifest: &Manifest,
    bytes: &[u8],
    entry: &str,
    params: &Value,
    options: RuntimeOptions,
) -> (H, Result<Value>) {
    run_call(host, manifest, bytes, entry, params, None, options)
}

pub(super) fn run_call<H: PluginHost + 'static>(
    host: H,
    manifest: &Manifest,
    bytes: &[u8],
    entry: &str,
    params: &Value,
    event: Option<&craft_plugin::ui::UiEvent>,
    options: RuntimeOptions,
) -> (H, Result<Value>) {
    run_mode(host, manifest, bytes, entry, params, event, options, false)
}

pub(super) fn dialog_fields<H: PluginHost + 'static>(
    host: H,
    manifest: &Manifest,
    bytes: &[u8],
    entry: &str,
    params: &Value,
    ceilings: craft_plugin::HostLimits,
) -> (H, Result<String>) {
    let mut manifest = manifest.clone();
    if let Err(error) = ceilings.validate() {
        return (host, Err(error));
    }
    if !manifest.granted_capabilities().iter().any(|c| c.starts_with("dialog@")) {
        return (host, Err(component_error("dialog@0.1 is not declared")));
    }
    let Some(runtime) = manifest.runtime.as_mut() else {
        return (host, Err(component_error("no runtime")));
    };
    if !runtime.entries.iter().any(|e| e.name == entry && e.dialog_hook) {
        return (host, Err(component_error("entry has no dialog hook")));
    }
    runtime.limits.fuel = runtime.limits.fuel.min(ceilings.max_fuel).min(10_000_000);
    runtime.limits.memory_mib = runtime.limits.memory_mib.min(ceilings.max_memory_mib).min(16);
    runtime.limits.deadline_ms = runtime.limits.deadline_ms.min(250);
    runtime.limits.stack_kib = runtime.limits.stack_kib.map(|kib| kib.min(512));
    for file in &mut runtime.permissions.files {
        file.write = false;
    }
    runtime.permissions.files.retain(|file| file.read);
    let (host, result) = run_mode(host, &manifest, bytes, entry, params, None, RuntimeOptions::default(), true);
    (host, result.and_then(|value| value.as_str().map(str::to_owned).ok_or_else(|| component_error("hook result is not text"))))
}

#[allow(clippy::too_many_arguments)]
fn run_mode<H: PluginHost + 'static>(
    mut host: H,
    manifest: &Manifest,
    bytes: &[u8],
    entry: &str,
    params: &Value,
    event: Option<&craft_plugin::ui::UiEvent>,
    options: RuntimeOptions,
    dialog: bool,
) -> (H, Result<Value>) {
    let prepared = (|| {
        // A direct runtime caller may not have used parse_manifest. Validate only this new
        // capability here, preserving older imports' existing validated-manifest contract.
        let design_capabilities: Vec<String> =
            manifest.capabilities.iter().filter(|capability| capability.split('@').next() == Some("design")).take(17).cloned().collect();
        craft_plugin::check_capabilities(&design_capabilities, "the host")?;
        let design_tokens = manifest.granted_capabilities().iter().any(|capability| capability.starts_with("design@"));
        let runtime = manifest
            .runtime
            .as_ref()
            .ok_or_else(|| HostError::InvalidManifest { field: "runtime".into(), reason: "no executable runtime".into() })?;
        runtime.validate()?;
        ui::validate_manifest(manifest, runtime)?;
        if let Some(event) = event {
            event.validate()?;
            if !ui::enabled(runtime) {
                return Err(HostError::PermissionDenied { permission: "ui capability is not declared".into() });
            }
        }
        if runtime.is_importer() && !dialog {
            return Err(HostError::InvalidInput { field: "entry".into(), reason: "an importer converts a file; open the file instead".into() });
        }
        if runtime.is_exporter() && !dialog {
            return Err(HostError::InvalidInput { field: "entry".into(), reason: "an exporter writes a file; export the design instead".into() });
        }
        if bytes.len() > runtime.max_component_bytes() {
            return Err(limit("component size"));
        }
        let entry = runtime
            .entries
            .iter()
            .find(|e| e.name == entry)
            .ok_or_else(|| HostError::InvalidInput { field: "entry".into(), reason: "unknown entry".into() })?;
        let params = serde_json::to_string(params).map_err(|e| trap(e.to_string()))?;
        if params.len() > MAX_JSON_BYTES {
            return Err(limit("parameters exceed 1 MiB"));
        }
        let stack = stack_bytes(&runtime.limits, options, false)?;
        let (engine, component) = compile(bytes, stack)?;
        check_dialog_export(manifest, &engine, &component)?;
        let commands = host.command_catalog().into_iter().filter(|s| runtime.commands.contains(&s.name)).map(|s| (s.name.clone(), s)).collect();
        let groups: BTreeSet<String> = host.granted_command_groups().intersection(&runtime.permissions.command_groups).cloned().collect();
        let mut folders = BTreeMap::new();
        for granted in host.granted_folders() {
            if let Some(request) = runtime.permissions.files.iter().find(|r| r.name == granted.name) {
                let dir = Dir::open_ambient_dir(&granted.folder, cap_std::ambient_authority())
                    .map_err(|e| HostError::PermissionDenied { permission: format!("files:{}: {e}", granted.name) })?;
                folders.insert(
                    granted.name,
                    Folder { dir, path: granted.folder, read: request.read && granted.read, write: request.write && granted.write },
                );
            }
        }
        if host.is_cancelled() {
            return Err(HostError::Cancelled);
        }
        let input_folders = folder_access::open(&host, manifest)?;
        host.begin_transaction()?;
        Ok((runtime, entry, params, (engine, component, stack), (commands, groups, design_tokens), (folders, input_folders)))
    })();
    let (runtime, entry, params, compiled, commands, (folders, input_folders)) = match prepared {
        Ok(prepared) => prepared,
        Err(error) => return (host, Err(error)),
    };
    let granted_capabilities = manifest.granted_capabilities();
    let forms = granted_capabilities
        .iter()
        .any(|c| c.split_once('@').is_some_and(|(name, version)| name == "forms" && craft_plugin::capability_version(version) == Some((0, 1))));
    let user_storage = granted_capabilities.iter().any(|c| c.starts_with("user-storage@"));
    let (mut state, outcome) = execute(
        host,
        runtime,
        entry,
        params,
        event,
        compiled,
        commands,
        (folders, input_folders),
        options,
        granted_capabilities,
        forms,
        user_storage,
        dialog,
    );
    let result = (|| match outcome {
        Ok(value) => match state.host.commit_transaction() {
            Ok(()) => match state.publish_writes() {
                Ok(()) => state.host.publish_committed().map(|()| value).map_err(|error| HostError::PermissionDenied {
                    permission: format!("user-storage: the approved files were published but the settings were not saved: {error}"),
                }),
                Err(error) => {
                    // Files may have changed, but the settings stay as they were. The transaction
                    // has committed, so only what was staged for publication is dropped.
                    state.host.discard_committed();
                    Err(error)
                }
            },
            Err(error) => {
                state.host.rollback_transaction()?;
                Err(error)
            }
        },
        Err(error) => {
            state.host.rollback_transaction()?;
            Err(error)
        }
    })();
    state.discard_writes();
    (state.host, result)
}

#[allow(clippy::too_many_arguments)]
fn execute<H: PluginHost + 'static>(
    host: H,
    runtime: &craft_plugin::RuntimeManifest,
    entry: &craft_plugin::Entry,
    params: String,
    event: Option<&craft_plugin::ui::UiEvent>,
    (engine, component, stack): (Engine, Component, usize),
    (commands, groups, design_tokens): (BTreeMap<String, CommandSchema>, BTreeSet<String>, bool),
    (folders, input_folders): (BTreeMap<String, Folder>, BTreeMap<String, Folder>),
    options: RuntimeOptions,
    granted_capabilities: Vec<String>,
    forms: bool,
    user_storage: bool,
    dialog: bool,
) -> (State<H>, Result<Value>) {
    let limits = &runtime.limits;
    // A long job runs for its fixed deadline, and reports its progress as often as it likes.
    let (deadline_ms, max_calls) = if entry.job { (options.job_deadline_ms, MAX_JOB_CALLS) } else { (limits.deadline_ms, MAX_CALLS) };
    let state = State {
        host,
        commands,
        groups,
        granted_capabilities,
        effect: if dialog { Effect::Read } else { entry.effect },
        user_storage,
        design_tokens,
        folders,
        input_folders,
        folder_bytes: 0,
        folder_files: BTreeSet::new(),
        staged: BTreeMap::new(),
        created_dirs: Vec::new(),
        clock: runtime.permissions.clock,
        forms: forms && !dialog,
        failed: None,
        calls: 0,
        max_calls,
        log_bytes: 0,
        deadline: Instant::now() + Duration::from_millis(deadline_ms),
        limits: store_limits(limits, 10_000),
        import: None,
        panels: runtime.ui.clone(),
    };
    let mut store = Store::new(&engine, state);
    let outcome = (|| {
        prepare_store(&mut store, limits)?;
        let wit = check_imports(&engine, &component)?;
        ui::check_imports(&engine, &component, runtime)?;
        if wit == Wit::V0_1 && ui::enabled(runtime) {
            return Err(component_error("ui requires the current WIT version"));
        }
        let response = with_interrupts(&engine, || match wit {
            Wit::V0_1 if dialog => v0_1::call_dialog(&engine, &mut store, &component, &entry.name, &params, runtime.is_importer()),
            Wit::V0_1 => v0_1::call_entry(&engine, &mut store, &component, &entry.name, &params),
            Wit::Current if dialog => {
                let mut linker = Linker::new(&engine);
                link_host_query(&mut linker, &store.data().granted_capabilities)?;
                folders::add_to_linker::<_, wasmtime::component::HasSelf<_>>(&mut linker, |s| s)?;
                if runtime.is_importer() {
                    importer_storage_bindings::ImporterPluginWithStorage::add_to_linker::<_, wasmtime::component::HasSelf<_>>(&mut linker, |s| s)?;
                } else if runtime.is_exporter() {
                    exporter_bindings::ExporterPlugin::add_to_linker::<_, wasmtime::component::HasSelf<_>>(&mut linker, |s| s)?;
                } else if ui::enabled(runtime) {
                    panel_bindings::PanelPlugin::add_to_linker::<_, wasmtime::component::HasSelf<_>>(&mut linker, |s| s)?;
                } else {
                    // Command hooks may also import design-state or forms; the hook-only WIT world stays unchanged.
                    bindings::CommandPlugin::add_to_linker::<_, wasmtime::component::HasSelf<_>>(&mut linker, |s| s)?;
                }
                let guest = dialog_bindings::DialogPlugin::instantiate(&mut store, &component, &linker)?;
                Ok(guest.craft_plugin_dialog_provider().call_update_fields(&mut store, &entry.name, &params)?.map_err(host_error))
            }
            Wit::Current if ui::enabled(runtime) => ui::call_entry(&engine, &mut store, &component, runtime, &entry.name, &params, event),
            Wit::Current => {
                let mut linker = Linker::new(&engine);
                link_host_query(&mut linker, &store.data().granted_capabilities)?;
                folders::add_to_linker::<_, wasmtime::component::HasSelf<_>>(&mut linker, |s| s)?;
                bindings::CommandPlugin::add_to_linker::<_, wasmtime::component::HasSelf<_>>(&mut linker, |s| s)?;
                let guest = bindings::CommandPlugin::instantiate(&mut store, &component, &linker)?;
                Ok(call_entry!(guest, &mut store, &entry.name, &params)?.map_err(host_error))
            }
        })?;
        if let Some(error) = store.data().failed.clone() {
            return Err(error);
        }
        let response = response.map_err(|e| execution_error(&e, store.get_fuel().ok(), limits, stack))?;
        let text = response?;
        if text.len() > MAX_JSON_BYTES {
            return Err(limit("the entry's result is larger than 1 MiB"));
        }
        if store.data().host.is_cancelled() {
            return Err(HostError::Cancelled);
        }
        if Instant::now() >= store.data().deadline {
            return Err(deadline());
        }
        let value = if dialog {
            Value::String(text)
        } else {
            serde_json::from_str(&text)
                .map_err(|e| HostError::InvalidInput { field: "result".into(), reason: format!("entry result is not JSON: {e}") })?
        };
        if let Ok(remaining) = store.get_fuel() {
            ::log::debug!(
                target: "craft_plugin_wasm",
                "plugin entry completed: entry={}, fuel_consumed={}, fuel_remaining={remaining}, fuel_limit={}",
                entry.name,
                limits.fuel.saturating_sub(remaining),
                limits.fuel
            );
        }
        Ok(value)
    })();
    // Fuel can run out between epoch ticks. If a hook has also crossed its deadline, report
    // the typed timeout even when the fuel trap was the first opportunity to stop it.
    let expired = dialog && Instant::now() >= store.data().deadline;
    (store.into_data(), outcome.map_err(|error| with_limits(if expired { deadline() } else { error }, deadline_ms)))
}

fn store_limits(limits: &Limits, table_elements: usize) -> StoreLimits {
    StoreLimitsBuilder::new()
        .memory_size(limits.memory_mib.saturating_mul(1024 * 1024))
        .instances(8)
        .memories(1)
        .tables(2)
        .table_elements(table_elements)
        .trap_on_grow_failure(true)
        .build()
}

/// Fuel, memory and the deadline and cancellation checks every epoch tick.
fn prepare_store<H: PluginHost + 'static>(store: &mut Store<State<H>>, limits: &Limits) -> Result<()> {
    store.limiter(|s| &mut s.limits);
    store.set_fuel(limits.fuel).map_err(|e| trap(e.to_string()))?;
    store.set_epoch_deadline(1);
    store.epoch_deadline_callback(|mut context| {
        let state = context.data_mut();
        if state.host.is_cancelled() {
            state.failed.get_or_insert(HostError::Cancelled);
            return Err(wasmtime::Error::msg("plugin cancelled"));
        }
        if Instant::now() >= state.deadline {
            state.failed.get_or_insert_with(deadline);
            return Err(wasmtime::Error::msg("plugin deadline exceeded"));
        }
        Ok(UpdateDeadline::Continue(1))
    });
    Ok(())
}

/// Runs `call` while a worker ticks the engine's epoch, so a loop that never calls the host still
/// meets the deadline and Cancel.
fn with_interrupts<T>(engine: &Engine, call: impl FnOnce() -> wasmtime::Result<T>) -> Result<wasmtime::Result<T>> {
    let (stop, stopped) = mpsc::channel::<()>();
    let timer_engine = engine.clone();
    let clock = std::thread::Builder::new()
        .name("plugin-interrupt".into())
        .spawn(move || {
            while matches!(stopped.recv_timeout(EPOCH_POLL), Err(mpsc::RecvTimeoutError::Timeout)) {
                timer_engine.increment_epoch();
            }
        })
        .map_err(|e| trap(format!("could not start interrupt worker: {e}")))?;
    let response = call();
    let _ = stop.send(());
    if clock.join().is_err() {
        return Err(trap("interrupt worker failed"));
    }
    Ok(response)
}

/// The most stack a command's manifest may ask for: a command runs on the caller's own thread.
/// An importer runs on a thread sized for its request, so it may ask for more.
const COMMAND_STACK_MAX_BYTES: usize = 1024 * 1024;

/// The guest stack: the manifest's `stack_kib` when it asks for one, else the app's default.
fn stack_bytes(limits: &Limits, options: RuntimeOptions, importer: bool) -> Result<usize> {
    let bytes = limits.stack_kib.map_or(options.stack_bytes, |kib| kib as usize * 1024);
    if !(64 * 1024..=64 * 1024 * 1024).contains(&bytes) {
        return Err(HostError::InvalidInput { field: "stack_bytes".into(), reason: "must be 64 KiB..64 MiB".into() });
    }
    if !importer && bytes > COMMAND_STACK_MAX_BYTES.max(options.stack_bytes) {
        return Err(limit("stack_kib: a command may ask for at most 1 MiB"));
    }
    Ok(bytes)
}

/// The first eight bytes of a core WebAssembly module; a component's header names layer 1 instead.
const CORE_MODULE_HEADER: [u8; 8] = *b"\0asm\x01\0\0\0";

/// A host that answers nothing: [`check_component`] links against it and never instantiates.
struct Inert;
impl PluginHost for Inert {
    fn command_catalog(&self) -> Vec<CommandSchema> {
        Vec::new()
    }
    fn run_command(&mut self, name: &str, _args: &Value) -> Result<Value> {
        Err(HostError::PermissionDenied { permission: format!("command {name} is not granted") })
    }
    fn begin_transaction(&mut self) -> Result<()> {
        Ok(())
    }
    fn commit_transaction(&mut self) -> Result<()> {
        Ok(())
    }
    fn rollback_transaction(&mut self) -> Result<()> {
        Ok(())
    }
    fn granted_folders(&self) -> Vec<craft_plugin::FileGrant> {
        Vec::new()
    }
    fn log(&mut self, _message: &str) {}
    fn progress(&mut self, _fraction: f32, _message: &str) {}
}

fn component_error(reason: impl Into<String>) -> HostError {
    HostError::InvalidInput { field: "component".into(), reason: reason.into() }
}

/// Optional exports are checked before either ordinary execution or a field hook can run.
fn check_dialog_export(manifest: &Manifest, engine: &Engine, component: &Component) -> Result<bool> {
    let declared = manifest.granted_capabilities().iter().any(|c| c.starts_with("dialog@"));
    let hook = manifest.runtime.as_ref().is_some_and(|r| r.entries.iter().any(|e| e.dialog_hook));
    let exported = component.component_type().exports(engine).any(|(name, _)| name.starts_with("craft:plugin/dialog-provider@"));
    if exported && !declared || hook && !exported {
        return Err(component_error("dialog-provider export requires dialog@0.1 and is required by dialog_hook entries"));
    }
    Ok(hook)
}

pub(super) fn check_component(manifest: &Manifest, bytes: &[u8], options: RuntimeOptions) -> Result<()> {
    let runtime =
        manifest.runtime.as_ref().ok_or_else(|| HostError::InvalidManifest { field: "runtime".into(), reason: "no executable runtime".into() })?;
    runtime.validate()?;
    ui::validate_manifest(manifest, runtime)?;
    if bytes.len() > runtime.max_component_bytes() {
        return Err(limit(&format!("component size: {} bytes is over {} MiB", bytes.len(), runtime.max_component_bytes() >> 20)));
    }
    if bytes.starts_with(&CORE_MODULE_HEADER) {
        return Err(component_error("this is a core WebAssembly module, not a component"));
    }
    let importer = runtime.is_importer();
    let (engine, component, _) = compile_fresh(bytes, stack_bytes(&runtime.limits, options, importer || runtime.is_exporter())?, cache().disk())?;
    let wit = check_imports(&engine, &component)?;
    ui::check_imports(&engine, &component, runtime)?;
    let link = |e: wasmtime::Error| component_error(format!("{e:#}"));
    let hook = check_dialog_export(manifest, &engine, &component)?;
    if wit == Wit::V0_1 {
        if ui::enabled(runtime) {
            return Err(component_error("ui requires the current WIT version"));
        }
        if runtime.is_exporter() {
            return Err(component_error("an exporter needs the exporter-plugin world from WIT 0.2"));
        }
        return v0_1::check_link::<Inert>(&engine, &component, importer, hook, &manifest.granted_capabilities()).map_err(link);
    }
    if hook {
        let mut linker = Linker::<State<Inert>>::new(&engine);
        link_host_query(&mut linker, &manifest.granted_capabilities()).map_err(link)?;
        folders::add_to_linker::<_, wasmtime::component::HasSelf<_>>(&mut linker, |s| s).map_err(link)?;
        if importer {
            importer_storage_bindings::ImporterPluginWithStorage::add_to_linker::<_, wasmtime::component::HasSelf<_>>(&mut linker, |s| s)
                .map_err(link)?;
        } else if runtime.is_exporter() {
            exporter_bindings::ExporterPlugin::add_to_linker::<_, wasmtime::component::HasSelf<_>>(&mut linker, |s| s).map_err(link)?;
        } else if ui::enabled(runtime)
            || manifest.capabilities.iter().any(|capability| capability.split_once('@').is_some_and(|(name, _)| name == "ui"))
        {
            panel_bindings::PanelPlugin::add_to_linker::<_, wasmtime::component::HasSelf<_>>(&mut linker, |s| s).map_err(link)?;
        } else {
            bindings::CommandPlugin::add_to_linker::<_, wasmtime::component::HasSelf<_>>(&mut linker, |s| s).map_err(link)?;
        }
        dialog_bindings::DialogPluginPre::new(linker.instantiate_pre(&component).map_err(link)?).map_err(link)?;
    }
    let mut linker = Linker::<State<Inert>>::new(&engine);
    folders::add_to_linker::<_, wasmtime::component::HasSelf<_>>(&mut linker, |s| s).map_err(link)?;
    link_host_query(&mut linker, &manifest.granted_capabilities()).map_err(link)?;
    if importer && !hook && component.component_type().exports(&engine).any(|(name, _)| name.starts_with("craft:plugin/dialog-provider@")) {
        user_storage::add_to_linker::<_, wasmtime::component::HasSelf<_>>(&mut linker, |s| s).map_err(link)?;
        importer_dialog_bindings::ImporterDialogPlugin::add_to_linker::<_, wasmtime::component::HasSelf<_>>(&mut linker, |s| s).map_err(link)?;
        importer_dialog_bindings::ImporterDialogPluginPre::new(linker.instantiate_pre(&component).map_err(link)?).map_err(link)?;
    } else if importer {
        importer_storage_bindings::ImporterPluginWithStorage::add_to_linker::<_, wasmtime::component::HasSelf<_>>(&mut linker, |s| s)
            .map_err(link)?;
        importer_storage_bindings::ImporterPluginWithStoragePre::new(linker.instantiate_pre(&component).map_err(link)?).map_err(link)?;
    } else if runtime.is_exporter() {
        exporter_bindings::ExporterPlugin::add_to_linker::<_, wasmtime::component::HasSelf<_>>(&mut linker, |s| s).map_err(link)?;
        exporter_bindings::ExporterPluginPre::new(linker.instantiate_pre(&component).map_err(link)?).map_err(link)?;
    } else if ui::enabled(runtime) || manifest.capabilities.iter().any(|capability| capability.split_once('@').is_some_and(|(name, _)| name == "ui"))
    {
        panel_bindings::PanelPlugin::add_to_linker::<_, wasmtime::component::HasSelf<_>>(&mut linker, |s| s).map_err(link)?;
        panel_bindings::PanelPluginPre::new(linker.instantiate_pre(&component).map_err(link)?).map_err(link)?;
    } else {
        bindings::CommandPlugin::add_to_linker::<_, wasmtime::component::HasSelf<_>>(&mut linker, |s| s).map_err(link)?;
        bindings::CommandPluginPre::new(linker.instantiate_pre(&component).map_err(link)?).map_err(link)?;
    }
    Ok(())
}

/// Which of Wasmtime's compilers made a component's code. Both run the same sandbox: fuel, epoch
/// interruption, the stack limit and the store limits hold under either.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tier {
    /// Winch, Wasmtime's baseline compiler. For a 55 MB importer it compiled about 18 times faster
    /// than Cranelift with about a third of the peak memory, and its code ran up to half as fast.
    Baseline,
    /// Cranelift, Wasmtime's optimising compiler. Only its code goes into the disk cache.
    Optimized,
}

struct Compiled {
    bytes: Arc<[u8]>,
    stack_bytes: usize,
    engine: Engine,
    component: Component,
    tier: Tier,
}

/// The most recently compiled component, whether a background thread is compiling it with
/// Cranelift, and Wasmtime's disk cache. Keeping the compiled code in memory lets the next call
/// start at once, and the disk cache lets the next session skip the compile. Every call still gets
/// a fresh instance and store.
struct Cache {
    compiled: Mutex<Option<Compiled>>,
    optimizing: AtomicBool,
    /// Wasmtime's compilation cache in the folder the app set ([`crate::set_compile_cache_dir`]),
    /// opened on first use, or none.
    disk: OnceLock<Option<wasmtime::Cache>>,
    /// Tests hold this to keep the background compile from starting until they let it.
    #[cfg(test)]
    gate: Mutex<()>,
}
impl Cache {
    const fn new() -> Self {
        Self {
            compiled: Mutex::new(None),
            optimizing: AtomicBool::new(false),
            disk: OnceLock::new(),
            #[cfg(test)]
            gate: Mutex::new(()),
        }
    }
    /// Wasmtime's own compilation cache, or none. Its key covers the component's bytes, Wasmtime's
    /// version and every compile setting (fuel, epoch interruption, the processor's features), so a
    /// changed plugin, a new SolidCraft or another setting never reads another one's code. An entry
    /// that is missing, damaged or from another version is compiled again; a folder that can't be
    /// created or written leaves the compile as it was.
    fn disk(&self) -> Option<wasmtime::Cache> {
        self.disk.get_or_init(|| COMPILE_CACHE_DIR.get().cloned().flatten().and_then(|dir| open_disk_cache(&dir.join("wasmtime")))).clone()
    }
}
#[cfg(not(test))]
static CACHE: Cache = Cache::new();

/// The process's cache; in tests, one per test thread, so tests running side by side don't share one.
fn cache() -> &'static Cache {
    #[cfg(test)]
    {
        /// Waits out the test's background Cranelift compile when the test's thread ends. Each test
        /// runs in a process of its own under CI, so a compile still running would otherwise race
        /// the process's exit: libgcc's exit-time teardown of its unwind-frame table then meets
        /// Wasmtime registering or releasing that compile's frames, and the process aborts after
        /// the test has passed.
        struct Own(&'static Cache);
        impl Drop for Own {
            fn drop(&mut self) {
                while self.0.optimizing.load(Ordering::Acquire) {
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
            }
        }
        thread_local! {
            static OWN: Own = Own(Box::leak(Box::new(Cache::new())));
        }
        OWN.with(|own| own.0)
    }
    #[cfg(not(test))]
    {
        &CACHE
    }
}

/// The folder the app chose for compiled components ([`crate::set_compile_cache_dir`]).
static COMPILE_CACHE_DIR: OnceLock<Option<PathBuf>> = OnceLock::new();

pub(super) fn set_compile_cache_dir(dir: Option<PathBuf>) -> bool {
    COMPILE_CACHE_DIR.set(dir).is_ok()
}

fn open_disk_cache(dir: &Path) -> Option<wasmtime::Cache> {
    let mut config = wasmtime::CacheConfig::new();
    config.with_directory(dir);
    match wasmtime::Cache::new(config) {
        Ok(cache) => Some(cache),
        Err(e) => {
            ::log::warn!(target: "craft_plugin_wasm", "compiled plugins are not kept between sessions: {e:#}");
            None
        }
    }
}

#[cfg(test)]
thread_local! {
    /// Tests that must run on one compiler's code set this; the caches are then left alone.
    static FORCED_TIER: std::cell::Cell<Option<Tier>> = const { std::cell::Cell::new(None) };
}

/// The component's code: from memory when this session compiled these bytes already; else
/// Cranelift's code from the disk cache when an earlier session put it there; else Winch compiles
/// it, so the first call waits about a second rather than the tens of seconds Cranelift takes for a
/// large importer, and [`optimize_in_background`] later swaps in Cranelift's code and caches it.
fn compile(bytes: &[u8], stack_bytes: usize) -> Result<(Engine, Component)> {
    #[cfg(test)]
    if let Some(tier) = FORCED_TIER.with(std::cell::Cell::get) {
        return compile_with(bytes, stack_bytes, tier);
    }
    let cache = cache();
    if let Ok(compiled) = cache.compiled.lock()
        && let Some(cached) = compiled.as_ref()
        && cached.stack_bytes == stack_bytes
        && cached.bytes.as_ref() == bytes
    {
        return Ok((cached.engine.clone(), cached.component.clone()));
    }
    let (engine, component, tier) = compile_fresh(bytes, stack_bytes, cache.disk())?;
    if let Ok(mut compiled) = cache.compiled.lock() {
        *compiled = Some(Compiled { bytes: Arc::from(bytes), stack_bytes, engine: engine.clone(), component: component.clone(), tier });
    }
    Ok((engine, component))
}

/// Cranelift's code from the disk cache when an earlier compile put it there for these bytes; else
/// Winch's; else, when Winch refuses the component (a WebAssembly feature it lacks on this
/// processor) or anything else goes wrong, Cranelift's, as before Winch, and cached.
fn compile_fresh(bytes: &[u8], stack_bytes: usize, disk: Option<wasmtime::Cache>) -> Result<(Engine, Component, Tier)> {
    if let Some(disk) = &disk
        && let Ok(engine) = engine_for(stack_bytes, Tier::Optimized, true, Some(disk.clone()))
        && optimized_marker(disk, &engine, bytes).is_some_and(|marker| marker.is_file())
    {
        // A hit loads in about a second. Should the entry be gone (deleted, or cleaned up when the
        // folder grew too large), this compiles with Cranelift as before and caches it again.
        let component = compile_component(&engine, bytes)?;
        return Ok((engine, component, Tier::Optimized));
    }
    match compile_on(bytes, stack_bytes, Tier::Baseline, true, None) {
        Ok((engine, component)) => Ok((engine, component, Tier::Baseline)),
        Err(error) => {
            ::log::debug!(target: "craft_plugin_wasm", "baseline compile refused the component, using Cranelift: {error}");
            let (engine, component) = optimize_and_cache(bytes, stack_bytes, true, disk.as_ref())?;
            Ok((engine, component, Tier::Optimized))
        }
    }
}

/// Compiles with Cranelift into the disk cache, and notes that the cache now holds these bytes'
/// code so the next session reads it instead of starting with Winch.
fn optimize_and_cache(bytes: &[u8], stack_bytes: usize, parallel: bool, disk: Option<&wasmtime::Cache>) -> Result<(Engine, Component)> {
    // Wasmtime counts a miss only once it wrote the entry, and a hit when the entry was there.
    let counts = |disk: &wasmtime::Cache| disk.cache_misses() + disk.cache_hits();
    let before = disk.map(counts);
    let (engine, component) = compile_on(bytes, stack_bytes, Tier::Optimized, parallel, disk.cloned())?;
    if let Some(disk) = disk
        && before.is_some_and(|before| counts(disk) > before)
        && let Some(marker) = optimized_marker(disk, &engine, bytes)
    {
        let written = marker.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|()| std::fs::write(&marker, b""));
        if let Err(e) = written {
            ::log::debug!(target: "craft_plugin_wasm", "could not note the cached plugin code at {}: {e}", marker.display());
        }
    }
    Ok((engine, component))
}

/// Where [`optimize_and_cache`] notes that the disk cache holds Cranelift's code for `bytes` under
/// `engine`'s settings: beside Wasmtime's own folder, whose cleanup removes files it didn't write.
/// The note only chooses which compiler to start with: Wasmtime still checks the entry itself, and
/// a stale note costs one Cranelift compile.
fn optimized_marker(disk: &wasmtime::Cache, engine: &Engine, bytes: &[u8]) -> Option<PathBuf> {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    engine.precompile_compatibility_hash().hash(&mut hasher);
    bytes.hash(&mut hasher);
    Some(disk.directory().parent()?.join("optimized").join(format!("{:016x}", hasher.finish())))
}

/// When the cached component is Winch's, compiles it again with Cranelift on a background thread,
/// puts that in the disk cache, and swaps it in when it is done, so later calls (and later
/// sessions) run at full speed. Calls meanwhile keep using Winch's code and never wait. Started
/// after a call finishes, so the compile does not compete with it. A failure keeps Winch's code,
/// which works.
pub(super) fn optimize_in_background() -> Option<std::thread::JoinHandle<()>> {
    let cache = cache();
    let (bytes, stack_bytes) = match cache.compiled.lock() {
        Ok(cached) => match cached.as_ref() {
            Some(cached) if cached.tier == Tier::Baseline => (cached.bytes.clone(), cached.stack_bytes),
            _ => return None,
        },
        Err(_) => return None,
    };
    let busy = &cache.optimizing;
    if busy.swap(true, Ordering::AcqRel) {
        return None;
    }
    /// Clears `busy` however the thread ends.
    struct Idle(&'static AtomicBool);
    impl Drop for Idle {
        fn drop(&mut self) {
            self.0.store(false, Ordering::Release);
        }
    }
    let idle = Idle(busy);
    let disk = cache.disk();
    let spawned = std::thread::Builder::new().name("plugin-optimize".into()).spawn(move || {
        let _idle = idle;
        #[cfg(test)]
        let _gate = cache.gate.lock();
        // On one thread: slower, but it leaves the other cores to the app and peaks at far less
        // memory than a compile spread over every core (5 GB for a 55 MB importer).
        match optimize_and_cache(&bytes, stack_bytes, false, disk.as_ref()) {
            Ok((engine, component)) => {
                if let Ok(mut cached) = cache.compiled.lock()
                    && let Some(cached) = cached.as_mut()
                    && Arc::ptr_eq(&cached.bytes, &bytes)
                    && cached.stack_bytes == stack_bytes
                {
                    (cached.engine, cached.component, cached.tier) = (engine, component, Tier::Optimized);
                }
            }
            Err(error) => ::log::debug!(target: "craft_plugin_wasm", "background Cranelift compile failed; keeping the baseline code: {error}"),
        }
    });
    // A thread that could not start drops `idle` with the closure, clearing `busy`.
    spawned.ok()
}

/// The parser exposes limit failures as messages rather than a public error kind.
fn component_parse_error(error: &wasmtime::wasmparser::BinaryReaderError) -> HostError {
    if error.message().contains("exceeds limit") {
        limit(&format!("component validation: {}", error.message()))
    } else {
        trap(format!("component parsing failed: {error}"))
    }
}

/// Read the outer interface before Wasmtime validation. Exporting an instance creates another
/// instance index, so its 1000-instance limit can precede our 4096-interface limit. The typed scan
/// still checks nested interfaces after compilation; this pass does not interpret private names.
fn check_outer_interfaces(bytes: &[u8]) -> Result<()> {
    use wasmtime::wasmparser::{Parser, Payload};
    let mut depth = 0usize;
    let mut imports = 0usize;
    let mut exports = 0usize;
    let mut found = BTreeSet::new();
    for payload in Parser::new(0).parse_all(bytes) {
        let payload = payload.map_err(|e| component_parse_error(&e))?;
        match payload {
            Payload::Version { .. } => depth += 1,
            Payload::End(_) => depth = depth.saturating_sub(1),
            Payload::ComponentImportSection(section) if depth == 1 => {
                for import in section {
                    let import = import.map_err(|e| component_parse_error(&e))?;
                    imports += 1;
                    note_version(import.name.name, &mut found)?;
                }
            }
            Payload::ComponentExportSection(section) if depth == 1 => {
                for export in section {
                    let export = export.map_err(|e| component_parse_error(&e))?;
                    exports += 1;
                    note_version(export.name.name, &mut found)?;
                }
            }
            _ => {}
        }
    }
    if imports > 4096 {
        return Err(limit("plugin import interface count or depth"));
    }
    if exports > 4096 {
        return Err(limit("plugin export interface count or depth"));
    }
    Ok(())
}

/// Compiles `bytes` with `tier`'s compiler on every core, without either cache.
#[cfg(test)]
fn compile_with(bytes: &[u8], stack_bytes: usize, tier: Tier) -> Result<(Engine, Component)> {
    compile_on(bytes, stack_bytes, tier, true, None)
}

/// Compiles `bytes` under the host's configuration with `tier`'s compiler, on every core or on the
/// calling thread alone, reading and filling `disk` when there is one.
fn compile_on(bytes: &[u8], stack_bytes: usize, tier: Tier, parallel: bool, disk: Option<wasmtime::Cache>) -> Result<(Engine, Component)> {
    let engine = engine_for(stack_bytes, tier, parallel, disk)?;
    let component = compile_component(&engine, bytes)?;
    Ok((engine, component))
}

fn engine_for(stack_bytes: usize, tier: Tier, parallel: bool, disk: Option<wasmtime::Cache>) -> Result<Engine> {
    let mut config = Config::new();
    config.cache(disk);
    config.parallel_compilation(parallel);
    config.consume_fuel(true).epoch_interruption(true).wasm_memory64(false).wasm_multi_memory(false).max_wasm_stack(stack_bytes);
    config.strategy(match tier {
        Tier::Baseline => wasmtime::Strategy::Winch,
        Tier::Optimized => wasmtime::Strategy::Cranelift,
    });
    // Wasmtime refuses a Wasm stack larger than its async stack, even for synchronous calls.
    config.async_stack_size(stack_bytes.saturating_add(1 << 20));
    Engine::new(&config).map_err(|e| trap(e.to_string()))
}

fn compile_component(engine: &Engine, bytes: &[u8]) -> Result<Component> {
    check_outer_interfaces(bytes)?;
    Component::from_binary(engine, bytes).map_err(|e| {
        // Inspect the parser error itself, not plugin names in the compilation context.
        if let Some(error) = e.downcast_ref::<wasmtime::wasmparser::BinaryReaderError>() {
            return component_parse_error(error);
        }
        trap(format!("component compilation failed: {e:#}"))
    })
}

enum ImportOutput {
    Design(Imported),
    Fields(Value),
}

pub(super) fn run_import<H: PluginHost + 'static>(
    host: H,
    manifest: &Manifest,
    bytes: &[u8],
    entry: &str,
    options: &Value,
    request: &ImportRequest,
    runtime_options: RuntimeOptions,
) -> (H, Result<Imported>) {
    let (host, result) = run_import_or_dialog(host, manifest, bytes, entry, options, request, runtime_options, false);
    (
        host,
        result.and_then(|output| match output {
            ImportOutput::Design(design) => Ok(design),
            ImportOutput::Fields(_) => Err(trap("unexpected dialog output")),
        }),
    )
}

pub(super) fn update_fields<H: PluginHost + 'static>(
    host: H,
    manifest: &Manifest,
    bytes: &[u8],
    entry: &str,
    context: &Value,
    request: &ImportRequest,
) -> (H, Result<Value>) {
    if !manifest.granted_capabilities().iter().any(|c| c.starts_with("dialog@")) {
        return (host, Err(HostError::PermissionDenied { permission: "dialog@0.1 was not requested".into() }));
    }
    if context.to_string().len() > 2 * craft_plugin::dialog::MAX_UPDATE_BYTES {
        return (host, Err(limit("dialog context exceeds 128 KiB")));
    }
    let mut bounded = manifest.clone();
    if let Some(runtime) = bounded.runtime.as_mut() {
        runtime.limits.fuel = runtime.limits.fuel.min(craft_plugin::dialog::MAX_FUEL);
        runtime.limits.deadline_ms = runtime.limits.deadline_ms.min(craft_plugin::dialog::DEADLINE_MS);
    }
    let (host, result) = run_import_or_dialog(host, &bounded, bytes, entry, context, request, RuntimeOptions::default(), true);
    (
        host,
        result.and_then(|output| match output {
            ImportOutput::Fields(fields) => Ok(fields),
            ImportOutput::Design(_) => Err(trap("unexpected import output")),
        }),
    )
}

#[allow(clippy::too_many_arguments)]
fn run_import_or_dialog<H: PluginHost + 'static>(
    mut host: H,
    manifest: &Manifest,
    bytes: &[u8],
    entry: &str,
    options: &Value,
    request: &ImportRequest,
    runtime_options: RuntimeOptions,
    dialog: bool,
) -> (H, Result<ImportOutput>) {
    let prepared = (|| {
        let runtime = manifest
            .runtime
            .as_ref()
            .ok_or_else(|| HostError::InvalidManifest { field: "runtime".into(), reason: "no executable runtime".into() })?;
        runtime.validate()?;
        ui::validate_manifest(manifest, runtime)?;
        if !runtime.is_importer() {
            return Err(HostError::InvalidInput { field: "entry".into(), reason: "this plugin is not an importer".into() });
        }
        if bytes.len() > runtime.max_component_bytes() {
            return Err(limit("component size"));
        }
        let entry = runtime
            .entries
            .iter()
            .find(|e| e.name == entry)
            .ok_or_else(|| HostError::InvalidInput { field: "entry".into(), reason: "unknown entry".into() })?;
        let options = serde_json::to_string(options).map_err(|e| trap(e.to_string()))?;
        if options.len() > MAX_JSON_BYTES {
            return Err(limit("options exceed 1 MiB"));
        }
        let name = request.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if name.is_empty() || name.len() > 255 || name.chars().any(char::is_control) || name.starts_with('.') {
            return Err(HostError::InvalidInput { field: "path".into(), reason: "choose a named, visible file".into() });
        }
        let folder = request.path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new(".")).to_path_buf();
        let stack = stack_bytes(&runtime.limits, runtime_options, true)?;
        let (engine, component) = compile(bytes, stack)?;
        check_dialog_export(manifest, &engine, &component)?;
        // Compilation is cached and happens outside the execution budget. Every source I/O
        // operation and guest execution share one deadline, including open/stat on a slow mount.
        let dialog_deadline = dialog.then(|| Instant::now() + Duration::from_millis(runtime.limits.deadline_ms));
        let filename = name.clone();
        let open = move || {
            let dir = Dir::open_ambient_dir(&folder, cap_std::ambient_authority())
                .map_err(|e| HostError::PermissionDenied { permission: format!("source: {}: {e}", folder.display()) })?;
            let size = match dir.symlink_metadata(&filename) {
                Ok(metadata) if metadata.is_file() => metadata.len(),
                Ok(_) => return Err(HostError::InvalidInput { field: "path".into(), reason: "not a regular file".into() }),
                Err(e) => return Err(HostError::InvalidInput { field: "path".into(), reason: e.to_string() }),
            };
            if size > MAX_IMPORT_INPUT_BYTES {
                return Err(limit("source: the file is larger than 1 GiB"));
            }
            let prefix = if dialog {
                let mut options = OpenOptions::new();
                options.read(true);
                #[cfg(unix)]
                options.custom_flags(O_NONBLOCK);
                let mut file = dir.open_with(&filename, &options).map_err(|e| trap(e.to_string()))?;
                if !file.metadata().map_err(|e| trap(e.to_string()))?.is_file() {
                    return Err(component_error("dialog source must be a regular file"));
                }
                let mut prefix = Vec::new();
                Read::by_ref(&mut file).take(craft_plugin::dialog::MAX_PREFIX_BYTES).read_to_end(&mut prefix).map_err(|e| trap(e.to_string()))?;
                Some((prefix, size))
            } else {
                None
            };
            Ok((dir, size, prefix))
        };
        let (dir, size, prefix) = if let Some(end) = dialog_deadline {
            let reply = start_prefix_read(prefix_reads(), open)?;
            wait_prefix(&reply, end).map_err(|e| with_limits(e, runtime.limits.deadline_ms))?
        } else {
            open()?
        };
        let import = ImportState {
            dialog: prefix,
            dir,
            name: name.clone(),
            input_folder: !dialog && request.input_folder && runtime.permissions.input_folder,
            read_bytes: 0,
            files: BTreeSet::new(),
            design: Vec::new(),
            evaluate: !dialog && manifest.granted_capabilities().iter().any(|capability| capability.starts_with("evaluate@")),
            scratch: Vec::new(),
            evaluating: false,
        };
        let input_folders = if dialog { BTreeMap::new() } else { folder_access::open(&host, manifest)? };
        Ok((runtime, entry, options, (engine, component, stack), import, source::FileInfo { name, size }, dialog_deadline, input_folders))
    })();
    let (runtime, entry, options, (engine, component, stack), import, file, dialog_deadline, input_folders) = match prepared {
        Ok(prepared) => prepared,
        Err(error) => return (host, Err(error)),
    };
    if host.is_cancelled() {
        return (host, Err(HostError::Cancelled));
    }
    let user_storage = manifest.granted_capabilities().iter().any(|c| c.starts_with("user-storage@"));
    if user_storage && let Err(error) = host.begin_transaction() {
        return (host, Err(error));
    }
    let limits = &runtime.limits;
    let state = State {
        host,
        commands: BTreeMap::new(),
        groups: BTreeSet::new(),
        granted_capabilities: manifest.granted_capabilities(),
        effect: if dialog { Effect::Read } else { Effect::Open },
        user_storage,
        design_tokens: false,
        folders: BTreeMap::new(),
        input_folders,
        folder_bytes: 0,
        folder_files: BTreeSet::new(),
        staged: BTreeMap::new(),
        created_dirs: Vec::new(),
        clock: runtime.permissions.clock,
        forms: false,
        failed: None,
        calls: 0,
        max_calls: if dialog { MAX_CALLS } else { MAX_IMPORT_CALLS },
        log_bytes: 0,
        deadline: dialog_deadline.unwrap_or_else(|| Instant::now() + Duration::from_millis(limits.deadline_ms)),
        // A large converter has a large indirect-call table.
        limits: store_limits(limits, 1_000_000),
        import: Some(import),
        panels: None,
    };
    let mut store = Store::new(&engine, state);
    let outcome = (|| {
        prepare_store(&mut store, limits)?;
        let wit = check_imports(&engine, &component)?;
        if dialog && wit != Wit::Current {
            return Err(component_error("dialog@0.1 needs the current WIT package"));
        }
        ui::check_imports(&engine, &component, runtime)?;
        let response = with_interrupts(&engine, || match wit {
            Wit::V0_1 => v0_1::call_import(&engine, &mut store, &component, &entry.name, &file, &options),
            Wit::Current if dialog => {
                let mut linker = Linker::new(&engine);
                link_host_query(&mut linker, &store.data().granted_capabilities)?;
                folders::add_to_linker::<_, wasmtime::component::HasSelf<_>>(&mut linker, |s| s)?;
                user_storage::add_to_linker::<_, wasmtime::component::HasSelf<_>>(&mut linker, |s| s)?;
                importer_dialog_bindings::ImporterDialogPlugin::add_to_linker::<_, wasmtime::component::HasSelf<_>>(&mut linker, |s| s)?;
                let guest = importer_dialog_bindings::ImporterDialogPlugin::instantiate(&mut store, &component, &linker)?;
                Ok(call_dialog!(guest, &mut store, &entry.name, &options)?.map_err(host_error))
            }
            Wit::Current => {
                let mut linker = Linker::new(&engine);
                link_host_query(&mut linker, &store.data().granted_capabilities)?;
                folders::add_to_linker::<_, wasmtime::component::HasSelf<_>>(&mut linker, |s| s)?;
                importer_storage_bindings::ImporterPluginWithStorage::add_to_linker::<_, wasmtime::component::HasSelf<_>>(&mut linker, |s| s)?;
                let guest = importer_storage_bindings::ImporterPluginWithStorage::instantiate(&mut store, &component, &linker)?;
                Ok(call_import!(guest, &mut store, &entry.name, &file, &options)?.map_err(host_error))
            }
        })?;
        if let Some(error) = store.data().failed.clone() {
            return Err(error);
        }
        let response = response.map_err(|e| execution_error(&e, store.get_fuel().ok(), limits, stack))?;
        let text = response?;
        if text.len() > MAX_JSON_BYTES {
            return Err(limit("the importer's loss report is larger than 1 MiB"));
        }
        if store.data().host.is_cancelled() {
            return Err(HostError::Cancelled);
        }
        if dialog {
            if text.len() > craft_plugin::dialog::MAX_UPDATE_BYTES {
                return Err(limit("dialog reply exceeds 64 KiB"));
            }
            if Instant::now() >= store.data().deadline {
                return Err(deadline());
            }
            let fields = serde_json::from_str(&text).map_err(|e| component_error(format!("dialog reply is not JSON: {e}")))?;
            return Ok(ImportOutput::Fields(fields));
        }
        let report: LossReport = serde_json::from_str(&text)
            .map_err(|e| HostError::InvalidInput { field: "loss_report".into(), reason: format!("the importer's loss report is not valid: {e}") })?;
        report.validate()?;
        let design = store.data_mut().import.as_mut().map(|import| std::mem::take(&mut import.design)).unwrap_or_default();
        if design.is_empty() {
            return Err(HostError::InvalidInput { field: "design".into(), reason: "the importer wrote no design".into() });
        }
        // A last host call can finish after the deadline, before the next epoch tick sees it.
        if Instant::now() >= store.data().deadline {
            return Err(deadline());
        }
        Ok(ImportOutput::Design(Imported { design, report }))
    })();
    let mut state = store.into_data();
    state.close_evaluation();
    // The app accepts and validates the design before committing the pending preferences.
    // Every execution failure, including an invalid report or missing design, discards them.
    let outcome = match outcome {
        Err(error) if user_storage => state.host.rollback_transaction().and(Err(error)),
        Ok(output) if dialog && user_storage => state.host.rollback_transaction().map(|()| output),
        outcome => outcome,
    };
    (state.host, outcome.map_err(|error| with_limits(error, limits.deadline_ms)))
}

/// Runs an export slot in a fresh instance. The plugin reads the design only through the
/// read-only commands its manifest lists, inside a transaction that is always rolled back, and
/// reaches no file: it returns the bytes, and the caller writes them where the user chose.
pub(super) fn run_export<H: PluginHost + 'static>(
    mut host: H,
    manifest: &Manifest,
    bytes: &[u8],
    slot: &str,
    options: &Value,
    runtime_options: RuntimeOptions,
) -> (H, Result<Vec<u8>>) {
    let prepared = (|| {
        let runtime = manifest
            .runtime
            .as_ref()
            .ok_or_else(|| HostError::InvalidManifest { field: "runtime".into(), reason: "no executable runtime".into() })?;
        runtime.validate()?;
        if !runtime.is_exporter() {
            return Err(HostError::InvalidInput { field: "format".into(), reason: "this plugin is not an exporter".into() });
        }
        if bytes.len() > runtime.max_component_bytes() {
            return Err(limit("component size"));
        }
        let entry = runtime
            .entries
            .iter()
            .find(|e| e.name == slot)
            .ok_or_else(|| HostError::InvalidInput { field: "format".into(), reason: "unknown export slot".into() })?;
        let options = serde_json::to_string(options).map_err(|e| trap(e.to_string()))?;
        if options.len() > MAX_JSON_BYTES {
            return Err(limit("options exceed 1 MiB"));
        }
        let stack = stack_bytes(&runtime.limits, runtime_options, true)?;
        let (engine, component) = compile(bytes, stack)?;
        check_dialog_export(manifest, &engine, &component)?;
        // Read-only commands the manifest lists, and no sensitive group: nothing reaches a file.
        let commands: BTreeMap<String, CommandSchema> = host
            .command_catalog()
            .into_iter()
            .filter(|s| runtime.commands.contains(&s.name) && s.effect == Effect::Read && s.group.is_none())
            .map(|s| (s.name.clone(), s))
            .collect();
        if host.is_cancelled() {
            return Err(HostError::Cancelled);
        }
        host.begin_transaction()?;
        Ok((runtime, entry, options, engine, component, commands, stack))
    })();
    let (runtime, entry, options, engine, component, commands, stack) = match prepared {
        Ok(prepared) => prepared,
        Err(error) => return (host, Err(error)),
    };
    let limits = &runtime.limits;
    let state = State {
        host,
        commands,
        groups: BTreeSet::new(),
        granted_capabilities: manifest.granted_capabilities(),
        effect: Effect::Read,
        user_storage: false,
        design_tokens: false,
        folders: BTreeMap::new(),
        input_folders: BTreeMap::new(),
        folder_bytes: 0,
        folder_files: BTreeSet::new(),
        staged: BTreeMap::new(),
        created_dirs: Vec::new(),
        clock: runtime.permissions.clock,
        forms: false,
        failed: None,
        calls: 0,
        max_calls: MAX_IMPORT_CALLS,
        log_bytes: 0,
        deadline: Instant::now() + Duration::from_millis(limits.deadline_ms),
        limits: store_limits(limits, 1_000_000),
        import: None,
        panels: None,
    };
    let mut store = Store::new(&engine, state);
    let outcome = (|| {
        prepare_store(&mut store, limits)?;
        check_imports(&engine, &component)?;
        let mut linker = Linker::new(&engine);
        exporter_bindings::ExporterPlugin::add_to_linker::<_, wasmtime::component::HasSelf<_>>(&mut linker, |s| s)
            .map_err(|e| trap(e.to_string()))?;
        let response = with_interrupts(&engine, || {
            let guest = exporter_bindings::ExporterPlugin::instantiate(&mut store, &component, &linker)?;
            call_export!(guest, &mut store, &entry.name, &options)
        })?;
        if let Some(error) = store.data().failed.clone() {
            return Err(error);
        }
        let response = response.map_err(|e| execution_error(&e, store.get_fuel().ok(), limits, stack))?;
        let file = response.map_err(host_error)?;
        if file.len() > MAX_EXPORT_FILE_BYTES {
            return Err(limit("export: a file of more than 256 MiB"));
        }
        if store.data().host.is_cancelled() {
            return Err(HostError::Cancelled);
        }
        // A last host call can finish after the deadline, before the next epoch tick sees it.
        if Instant::now() >= store.data().deadline {
            return Err(deadline());
        }
        Ok(file)
    })();
    let mut state = store.into_data();
    // An export never changes the app: what its commands did is always thrown away.
    let rolled_back = state.host.rollback_transaction();
    let outcome = rolled_back.and(outcome);
    (state.host, outcome.map_err(|error| with_limits(error, limits.deadline_ms)))
}

#[cfg(test)]
mod tests;
pub(crate) mod ui;
mod v0_1;
