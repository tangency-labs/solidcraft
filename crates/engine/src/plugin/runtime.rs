//! Native command-component sandbox. No WASI is linked, even for a trusted package.
use super::{MAX_JSON_BYTES, Package, allowed_command, error};
use crate::{Result, Session};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::mpsc;
use std::time::{Duration, Instant};
use wasmtime::component::{Component, Linker};
use wasmtime::{Config, Engine, Store, StoreLimits, StoreLimitsBuilder};

mod bindings {
    wasmtime::component::bindgen!({ path: "../../wit", world: "command-plugin" });
}
use bindings::solidcraft::plugin::{engine, log, store, types::PluginError};

struct State {
    session: Session,
    commands: BTreeSet<String>,
    limits: StoreLimits,
    failed: Option<String>,
    calls: usize,
    log_bytes: usize,
    deadline: Instant,
}
impl State {
    fn refuse(&mut self, message: &str) -> PluginError {
        self.failed.get_or_insert_with(|| message.to_string());
        PluginError::Denied(message.to_string())
    }
}
impl bindings::solidcraft::plugin::types::Host for State {}
impl engine::Host for State {
    fn run(&mut self, command: String, args: String) -> std::result::Result<String, PluginError> {
        if self.failed.is_some() {
            return Err(self.refuse("transaction already failed"));
        }
        self.calls += 1;
        if self.calls > 256 || Instant::now() >= self.deadline || args.len() > MAX_JSON_BYTES {
            return Err(self.refuse("command count, JSON or deadline limit"));
        }
        if !self.commands.contains(&command) || !allowed_command(&command) {
            return Err(self.refuse("command is not granted"));
        }
        let params: Value = serde_json::from_str(&args).map_err(|_| self.refuse("invalid command JSON"))?;
        let before = self.session.doc.clone();
        let previous: BTreeMap<_, _> = self.session.model.results.iter().map(|r| (r.id, r.error.clone())).collect();
        // This also contains implicit reads during rebuild (fonts, linked designs, etc.).
        let reply = solidcraft_io::vfs::scoped(Some(Default::default()), || self.session.execute(&command, &params)).map_err(|e| {
            let e = e.to_string();
            self.failed = Some(e.clone());
            PluginError::Failed(e)
        })?;
        if let Some(failed) = self
            .session
            .model
            .results
            .iter()
            .find(|r| r.error.is_some() && (previous.get(&r.id) != Some(&r.error) || before.feature(r.id) != self.session.doc.feature(r.id)))
        {
            let message = failed.error.clone().unwrap_or_default();
            self.failed = Some(message.clone());
            return Err(PluginError::Failed(message));
        }
        let text = serde_json::to_string(&reply).map_err(|_| self.refuse("invalid command result"))?;
        if text.len() > MAX_JSON_BYTES {
            return Err(self.refuse("command result exceeds 1 MiB"));
        }
        Ok(text)
    }
}
impl log::Host for State {
    fn log(&mut self, _level: log::Level, message: String) {
        if message.len() > 1024 || self.log_bytes.saturating_add(message.len()) > 64 * 1024 {
            self.refuse("log limit exceeded");
            return;
        }
        self.log_bytes += message.len();
        self.session.echo(format!("plugin: {}", message.chars().filter(|c| !c.is_control()).collect::<String>()));
    }
}
impl store::Host for State {
    fn get(&mut self, _: String) -> std::result::Result<Option<Vec<u8>>, PluginError> {
        Err(self.refuse("persistent store is not supported"))
    }
    fn set(&mut self, _: String, _: Vec<u8>) -> std::result::Result<(), PluginError> {
        Err(self.refuse("persistent store is not supported"))
    }
    fn remove(&mut self, _: String) -> std::result::Result<(), PluginError> {
        Err(self.refuse("persistent store is not supported"))
    }
    fn keys(&mut self) -> std::result::Result<Vec<String>, PluginError> {
        Err(self.refuse("persistent store is not supported"))
    }
    fn previous_version(&mut self) -> Option<String> {
        None
    }
}

// The outer Display can contain only a Wasm backtrace. Keep the root cause first,
// plus the typed trap and exact fuel counters, without relaxing any execution limit.
pub(super) fn execution_error(failure: &wasmtime::Error, remaining: Option<u64>, limits: &super::Limits) -> crate::EngineError {
    let trap = failure.downcast_ref::<wasmtime::Trap>().map(|trap| format!("{trap:?}")).unwrap_or_else(|| "none".into());
    let fuel = remaining
        .map(|remaining| format!("fuel_consumed={}, fuel_remaining={remaining}", limits.fuel.saturating_sub(remaining)))
        .unwrap_or_else(|| "fuel counters unavailable".into());
    error(format!(
        "component execution failed: {} (trap={trap}, {fuel}, fuel_limit={}, memory_limit_mib={}, deadline_ms={}): {failure:#}",
        failure.root_cause(),
        limits.fuel,
        limits.memory_mib,
        limits.deadline_ms
    ))
}

pub(super) fn run(session: &mut Session, package: &Package, entry: &str, params: &Value) -> Result<Value> {
    let runtime = package.manifest.runtime.as_ref().ok_or_else(|| error("no runtime"))?;
    let params = serde_json::to_string(params).map_err(|e| error(e.to_string()))?;
    if params.len() > MAX_JSON_BYTES {
        return Err(error("parameters exceed 1 MiB"));
    }
    let mut config = Config::new();
    config.consume_fuel(true).epoch_interruption(true).wasm_memory64(false).wasm_multi_memory(false).max_wasm_stack(512 * 1024);
    let engine = Engine::new(&config).map_err(|e| error(e.to_string()))?;
    // Compilation is native work, outside guest fuel and deadline. Size is capped by read_package.
    // Never deserialize executable cache bytes from a package or a design.
    let component = Component::from_binary(&engine, &package.component).map_err(|e| error(format!("component compilation failed: {e}")))?;
    let limit = Duration::from_millis(runtime.limits.deadline_ms);
    let state = State {
        session: session.scratch(),
        commands: runtime.commands.clone(),
        failed: None,
        calls: 0,
        log_bytes: 0,
        limits: StoreLimitsBuilder::new()
            .memory_size(runtime.limits.memory_mib.checked_mul(1024 * 1024).ok_or_else(|| error("memory limit cannot be represented on this host"))?)
            .instances(8)
            .memories(1)
            .tables(2)
            .table_elements(10_000)
            .trap_on_grow_failure(true)
            .build(),
        deadline: Instant::now() + limit,
    };
    let mut store = Store::new(&engine, state);
    store.limiter(|s| &mut s.limits);
    store.set_fuel(runtime.limits.fuel).map_err(|e| error(e.to_string()))?;
    store.set_epoch_deadline(1);
    let mut linker = Linker::new(&engine);
    bindings::CommandPlugin::add_to_linker::<_, wasmtime::component::HasSelf<_>>(&mut linker, |s| s).map_err(|e| error(e.to_string()))?;
    let (stop, stopped) = mpsc::channel::<()>();
    let timer_engine = engine.clone();
    let clock = std::thread::Builder::new()
        .name("plugin-deadline".into())
        .spawn(move || {
            if stopped.recv_timeout(limit).is_err() {
                timer_engine.increment_epoch();
            }
        })
        .map_err(|e| error(format!("could not start the deadline worker: {e}")))?;
    let response = (|| {
        let guest = bindings::CommandPlugin::instantiate(&mut store, &component, &linker)?;
        guest.solidcraft_plugin_command().call_execute(&mut store, entry, &params)
    })();
    let _ = stop.send(());
    if clock.join().is_err() {
        return Err(error("deadline worker failed"));
    }
    let response = response.map_err(|e| execution_error(&e, store.get_fuel().ok(), &runtime.limits))?;
    let text = response.map_err(|e| error(format!("entry failed: {e:?}")))?;
    if text.len() > MAX_JSON_BYTES {
        return Err(error("entry result exceeds 1 MiB"));
    }
    let value = serde_json::from_str(&text).map_err(|e| error(format!("entry result is not JSON: {e}")))?;
    let remaining = store.get_fuel().ok();
    let mut state = store.into_data();
    if let Some(message) = state.failed {
        return Err(error(message));
    }
    if Instant::now() >= state.deadline {
        return Err(error("entry deadline exceeded"));
    }
    if let Some(remaining) = remaining {
        ::log::debug!(
            "plugin execution completed: fuel_consumed={}, fuel_remaining={remaining}, fuel_limit={}",
            runtime.limits.fuel.saturating_sub(remaining),
            runtime.limits.fuel
        );
    }
    // Nested commands ran only on the scratch session. The outer plugin.run records one step.
    state.session.undo.clear();
    state.session.redo.clear();
    if !std::sync::Arc::ptr_eq(&session.doc, &state.session.doc)
        || session.active_sketch != state.session.active_sketch
        || session.pending_moves != state.session.pending_moves
    {
        session.accept_computed(state.session);
    } else {
        for message in state.session.log {
            session.echo(message);
        }
    }
    Ok(value)
}
