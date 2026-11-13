use crate::RuntimeOptions;
use cap_std::fs::{Dir, OpenOptions};
use craft_plugin::{CommandSchema, Effect, HostError, Limits, MAX_COMPONENT_BYTES, MAX_JSON_BYTES, Manifest, PluginHost, Result};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::path::{Component as PathComponent, Path, PathBuf};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    mpsc,
};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use wasmtime::component::{Component, Linker};
use wasmtime::{Config, Engine, Store, StoreLimits, StoreLimitsBuilder, UpdateDeadline};

mod bindings {
    wasmtime::component::bindgen!({ path: "../craft-plugin/wit", world: "command-plugin" });
}
include!(concat!(env!("OUT_DIR"), "/aliases.rs"));

const MAX_CALLS: usize = 256;
const MAX_LOG_BYTES: usize = 64 * 1024;
const EPOCH_POLL: Duration = Duration::from_millis(10);
static NEXT_FILE: AtomicU64 = AtomicU64::new(0);

fn limit(name: &str) -> HostError {
    HostError::LimitExceeded { limit: name.into() }
}
fn trap(reason: impl Into<String>) -> HostError {
    HostError::Trap { reason: reason.into() }
}
fn wit_error(error: HostError) -> PluginError {
    match error {
        HostError::PermissionDenied { permission } => PluginError::PermissionDenied(permission),
        HostError::CommandFailed { command, reason } => PluginError::CommandFailed(types::CommandFailure { command, reason }),
        HostError::LimitExceeded { limit } => PluginError::LimitExceeded(limit),
        HostError::InvalidManifest { field, reason } => PluginError::InvalidManifest(types::FieldFailure { field, reason }),
        HostError::InvalidInput { field, reason } => PluginError::InvalidInput(types::FieldFailure { field, reason }),
        HostError::Trap { reason } => PluginError::Trap(reason),
        HostError::Cancelled => PluginError::Cancelled,
        HostError::UnsupportedCapability { reason, .. } => PluginError::InvalidManifest(types::FieldFailure { field: "capabilities".into(), reason }),
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
    effect: Effect,
    folders: BTreeMap<String, Folder>,
    staged: BTreeMap<(String, PathBuf), PendingWrite>,
    clock: bool,
    limits: StoreLimits,
    failed: Option<HostError>,
    calls: usize,
    log_bytes: usize,
    deadline: Instant,
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
            return Err(self.fail(limit("deadline")));
        }
        self.calls = self.calls.saturating_add(1);
        if self.calls > MAX_CALLS {
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
        Ok(())
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
impl<H: PluginHost> log::Host for State<H> {
    fn log(&mut self, _level: log::Level, message: String) {
        if self.check().is_err() {
            return;
        }
        self.log_bytes = self.log_bytes.saturating_add(message.len());
        if message.len() > 1024 || self.log_bytes > MAX_LOG_BYTES {
            self.fail(limit("log size"));
            return;
        }
        self.host.log(&message.chars().filter(|c| !c.is_control()).collect::<String>());
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
impl<H: PluginHost> clock::Host for State<H> {
    fn now(&mut self) -> u64 {
        if !self.clock || self.check().is_err() {
            return 0;
        }
        SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or_default()
    }
}
impl<H: PluginHost> files::Host for State<H> {
    fn read_file(&mut self, grant: String, path: String) -> std::result::Result<Vec<u8>, PluginError> {
        self.file_path(&grant, &path, false)?;
        let read = (|| -> std::io::Result<Vec<u8>> {
            let folder = self.folders.get(&grant).ok_or_else(|| std::io::Error::other("grant disappeared"))?;
            let staged = self.staged.get(&(grant.clone(), Path::new(&path).components().collect()));
            let file = folder.dir.open(staged.map(|pending| pending.temporary.as_path()).unwrap_or(Path::new(&path)))?;
            if !file.metadata()?.is_file() {
                return Err(std::io::Error::other("only regular files are supported"));
            }
            let mut bytes = Vec::new();
            file.take(MAX_JSON_BYTES as u64 + 1).read_to_end(&mut bytes)?;
            Ok(bytes)
        })();
        let bytes = read.map_err(|e| self.fail(HostError::PermissionDenied { permission: format!("files.read:{grant}: {e}") }))?;
        if bytes.len() > MAX_JSON_BYTES {
            return Err(self.fail(limit("file read size")));
        }
        Ok(bytes)
    }
    fn write_file(&mut self, grant: String, path: String, data: Vec<u8>) -> std::result::Result<(), PluginError> {
        self.file_path(&grant, &path, true)?;
        let path: PathBuf = Path::new(&path).components().collect();
        if data.len() > MAX_JSON_BYTES {
            return Err(self.fail(limit("file write size")));
        }
        let write = (|| -> std::io::Result<PathBuf> {
            let folder = self.folders.get(&grant).ok_or_else(|| std::io::Error::other("grant disappeared"))?;
            let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
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
        let temporary = write.map_err(|e| self.fail(HostError::PermissionDenied { permission: format!("files.write:{grant}: {e}") }))?;
        if let Some(previous) = self.staged.insert((grant.clone(), path), PendingWrite { temporary, order: self.calls })
            && let Some(folder) = self.folders.get(&grant)
        {
            let _ = folder.dir.remove_file(previous.temporary);
        }
        Ok(())
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
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn execution_error(failure: &wasmtime::Error, remaining: Option<u64>, limits: &Limits) -> HostError {
    let trap_name = failure.downcast_ref::<wasmtime::Trap>().map(|t| format!("{t:?}")).unwrap_or_else(|| "none".into());
    let fuel = remaining.map(|n| format!("fuel_consumed={}, fuel_remaining={n}", limits.fuel.saturating_sub(n))).unwrap_or_default();
    let reason = format!(
        "component execution failed: {} (trap={trap_name}, {fuel}, fuel_limit={}, memory_limit_mib={}, deadline_ms={}): {failure:#}",
        failure.root_cause(),
        limits.fuel,
        limits.memory_mib,
        limits.deadline_ms
    );
    match failure.downcast_ref::<wasmtime::Trap>() {
        Some(wasmtime::Trap::OutOfFuel) => limit(&format!("fuel: {reason}")),
        Some(wasmtime::Trap::StackOverflow) => limit(&format!("stack: {reason}")),
        _ if reason.contains("growing memory") => limit(&format!("memory: {reason}")),
        _ => trap(reason),
    }
}

/// Refuse reserved network imports even when Wasmtime elides an unused import instance.
fn reject_network_imports(engine: &Engine, component: &Component) -> Result<()> {
    use wasmtime::component::types::ComponentItem;
    fn check(engine: &Engine, name: &str, item: ComponentItem, depth: usize, remaining: &mut usize) -> Result<()> {
        if depth > 64 || *remaining == 0 {
            return Err(limit("plugin import interface count or depth"));
        }
        *remaining -= 1;
        let package = craft_plugin::API.split_once('@').map(|(package, _)| package).unwrap_or(craft_plugin::API);
        if let Some(version) = name.strip_prefix(&format!("{package}/")).and_then(|rest| rest.split_once('@')).map(|(_, version)| version)
            && Some(version) != craft_plugin::API.split_once('@').map(|(_, version)| version)
        {
            let version: String = version.chars().take(32).collect();
            return Err(HostError::UnsupportedCapability {
                capability: format!("{package}@{version}"),
                reason: format!("built against {package}@{version}, and this host runs {}; update the app or the plugin", craft_plugin::API),
            });
        }
        if name == "fetch" || name.starts_with("wasi:sockets/") || name.starts_with("wasi:http/") || name.starts_with(&format!("{package}/fetch")) {
            return Err(HostError::PermissionDenied { permission: format!("network import is forbidden: {name}") });
        }
        if let ComponentItem::ComponentInstance(instance) = item {
            for (name, export) in instance.exports(engine) {
                check(engine, name, export.ty, depth + 1, remaining)?;
            }
        }
        Ok(())
    }
    let mut remaining = 4096;
    for (name, import) in component.component_type().imports(engine) {
        check(engine, name, import.ty, 0, &mut remaining)?;
    }
    Ok(())
}

pub(super) fn run<H: PluginHost + 'static>(
    mut host: H,
    manifest: &Manifest,
    bytes: &[u8],
    entry: &str,
    params: &Value,
    options: RuntimeOptions,
) -> (H, Result<Value>) {
    let prepared = (|| {
        if bytes.len() > MAX_COMPONENT_BYTES {
            return Err(limit("component size"));
        }
        let runtime = manifest
            .runtime
            .as_ref()
            .ok_or_else(|| HostError::InvalidManifest { field: "runtime".into(), reason: "no executable runtime".into() })?;
        runtime.validate()?;
        let entry = runtime
            .entries
            .iter()
            .find(|e| e.name == entry)
            .ok_or_else(|| HostError::InvalidInput { field: "entry".into(), reason: "unknown entry".into() })?;
        if !(64 * 1024..=64 * 1024 * 1024).contains(&options.stack_bytes) {
            return Err(HostError::InvalidInput { field: "stack_bytes".into(), reason: "must be 64 KiB..64 MiB".into() });
        }
        let params = serde_json::to_string(params).map_err(|e| trap(e.to_string()))?;
        if params.len() > MAX_JSON_BYTES {
            return Err(limit("parameters exceed 1 MiB"));
        }
        let mut config = Config::new();
        config.consume_fuel(true).epoch_interruption(true).wasm_memory64(false).wasm_multi_memory(false).max_wasm_stack(options.stack_bytes);
        let engine = Engine::new(&config).map_err(|e| trap(e.to_string()))?;
        let component = Component::from_binary(&engine, bytes).map_err(|e| trap(format!("component compilation failed: {e:#}")))?;
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
        host.begin_transaction()?;
        Ok((runtime, entry, params, engine, component, (commands, groups), folders))
    })();
    let (runtime, entry, params, engine, component, commands, folders) = match prepared {
        Ok(prepared) => prepared,
        Err(error) => return (host, Err(error)),
    };
    let (mut state, outcome) = execute(host, runtime, entry, params, engine, component, commands, folders);
    let result = (|| match outcome {
        Ok(value) => match state.host.commit_transaction() {
            Ok(()) => state.publish_writes().map(|()| value),
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
    engine: Engine,
    component: Component,
    (commands, groups): (BTreeMap<String, CommandSchema>, BTreeSet<String>),
    folders: BTreeMap<String, Folder>,
) -> (State<H>, Result<Value>) {
    let limits = &runtime.limits;
    let state = State {
        host,
        commands,
        groups,
        effect: entry.effect,
        folders,
        staged: BTreeMap::new(),
        clock: runtime.permissions.clock,
        failed: None,
        calls: 0,
        log_bytes: 0,
        deadline: Instant::now() + Duration::from_millis(limits.deadline_ms),
        limits: StoreLimitsBuilder::new()
            .memory_size(limits.memory_mib.saturating_mul(1024 * 1024))
            .instances(8)
            .memories(1)
            .tables(2)
            .table_elements(10_000)
            .trap_on_grow_failure(true)
            .build(),
    };
    let mut store = Store::new(&engine, state);
    let outcome = (|| {
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
                state.failed.get_or_insert_with(|| limit("deadline"));
                return Err(wasmtime::Error::msg("plugin deadline exceeded"));
            }
            Ok(UpdateDeadline::Continue(1))
        });
        reject_network_imports(&engine, &component)?;
        let mut linker = Linker::new(&engine);
        bindings::CommandPlugin::add_to_linker::<_, wasmtime::component::HasSelf<_>>(&mut linker, |s| s).map_err(|e| trap(e.to_string()))?;
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
        let response = (|| {
            let guest = bindings::CommandPlugin::instantiate(&mut store, &component, &linker)?;
            call_entry!(guest, &mut store, &entry.name, &params)
        })();
        let _ = stop.send(());
        if clock.join().is_err() {
            return Err(trap("interrupt worker failed"));
        }
        if let Some(error) = store.data().failed.clone() {
            return Err(error);
        }
        let response = response.map_err(|e| execution_error(&e, store.get_fuel().ok(), limits))?;
        let text = response.map_err(host_error)?;
        if text.len() > MAX_JSON_BYTES {
            return Err(limit("entry result exceeds 1 MiB"));
        }
        if store.data().host.is_cancelled() {
            return Err(HostError::Cancelled);
        }
        if Instant::now() >= store.data().deadline {
            return Err(limit("deadline"));
        }
        let value = serde_json::from_str(&text)
            .map_err(|e| HostError::InvalidInput { field: "result".into(), reason: format!("entry result is not JSON: {e}") })?;
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
    (store.into_data(), outcome)
}

#[cfg(test)]
mod tests;
