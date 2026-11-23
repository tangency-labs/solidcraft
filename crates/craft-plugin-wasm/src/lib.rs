//! An app-independent component sandbox. Only explicitly bound Craft imports exist.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use craft_plugin::{LossReport, Manifest, PluginHost, Result};
use serde_json::Value;
use std::path::PathBuf;

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
mod native;

/// Trusted app configuration, separate from the untrusted package's budgets.
#[derive(Clone, Copy, Debug)]
pub struct RuntimeOptions {
    pub stack_bytes: usize,
}
impl Default for RuntimeOptions {
    fn default() -> Self {
        Self { stack_bytes: 512 * 1024 }
    }
}

/// Keeps compiled components in `dir` between sessions, with Wasmtime's own compilation cache, so
/// a large plugin compiles once rather than in every session. Trusted app configuration: the folder
/// is the app's per-user cache, never a plugin's or a project's. Set it once, before the first
/// call; `None`, or never calling this, keeps compiled code in memory only. Returns `false` when
/// it was already set. Anyone who can write to `dir` can change the machine code a plugin call
/// runs, so it must be a folder only the user can write.
pub fn set_compile_cache_dir(dir: Option<PathBuf>) -> bool {
    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    {
        native::set_compile_cache_dir(dir)
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        let _ = dir;
        false
    }
}

/// Runs with an owned host because Wasmtime store data must be `'static`.
/// Returns the host on success and failure, after committing or rolling back.
pub fn run<H: PluginHost + 'static>(host: H, manifest: &Manifest, component: &[u8], entry: &str, params: &Value) -> (H, Result<Value>) {
    run_with_options(host, manifest, component, entry, params, RuntimeOptions::default())
}

pub fn run_with_options<H: PluginHost + 'static>(
    host: H,
    manifest: &Manifest,
    component: &[u8],
    entry: &str,
    params: &Value,
    options: RuntimeOptions,
) -> (H, Result<Value>) {
    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    {
        native::run(host, manifest, component, entry, params, options)
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        let _ = (manifest, component, entry, params, options);
        (host, Err(craft_plugin::HostError::Trap { reason: "component execution requires an x86-64 or AArch64 native build".into() }))
    }
}

/// Run one read-only field hook with independent small budgets, capped by approved ceilings.
pub fn dialog_fields<H: PluginHost + 'static>(
    host: H,
    manifest: &Manifest,
    component: &[u8],
    entry: &str,
    values: &Value,
    ceilings: craft_plugin::HostLimits,
) -> (H, Result<String>) {
    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    {
        native::dialog_fields(host, manifest, component, entry, values, ceilings)
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        let _ = (manifest, component, entry, values, ceilings);
        (host, Err(craft_plugin::HostError::Trap { reason: "component execution requires a native build".into() }))
    }
}

/// Checks `component` against `manifest` the way a call would, without running any of its code:
/// the runtime declaration, the component's size, that it is a component (not a core module) that
/// compiles, imports nothing but this host's interfaces at its version (never WASI or the network),
/// and exports the interfaces of its world (`command-plugin`, `panel-plugin`, or
/// `importer-plugin`). The error names what is wrong; `plugin check` adds how to fix it.
pub fn check_component(manifest: &Manifest, component: &[u8]) -> Result<()> {
    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    {
        native::check_component(manifest, component, RuntimeOptions::default())
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        let _ = (manifest, component);
        Err(craft_plugin::HostError::Trap { reason: "checking a component requires an x86-64 or AArch64 native build".into() })
    }
}

/// One file for an importer to convert. The app has already checked that it may read `path`.
#[derive(Clone, Debug)]
pub struct ImportRequest {
    /// The file being imported. The importer sees only its name, never the folders above it.
    pub path: PathBuf,
    /// The user granted the manifest's `input_folder` permission: the importer may read the other
    /// files in `path`'s folder (an assembly's parts) during this call.
    pub input_folder: bool,
}

/// What a successful import made: a SolidCraft design file and what could not be carried over.
#[derive(Clone, Debug)]
pub struct Imported {
    pub design: Vec<u8>,
    pub report: LossReport,
}

/// Runs an importer entry on one file in a fresh instance. It touches no app state, so there is
/// no transaction: the app opens the design as a new one only when the whole call succeeds.
/// A trap fails this call alone.
pub fn run_import<H: PluginHost + 'static>(
    host: H,
    manifest: &Manifest,
    component: &[u8],
    entry: &str,
    options: &Value,
    request: &ImportRequest,
) -> (H, Result<Imported>) {
    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    {
        native::run_import(host, manifest, component, entry, options, request, RuntimeOptions::default())
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        let _ = (manifest, component, entry, options, request);
        (host, Err(craft_plugin::HostError::Trap { reason: "component execution requires an x86-64 or AArch64 native build".into() }))
    }
}

/// Read and validate a panel plugin's static declarations with command and file authority disabled.
/// The caller retains the host on failure as well as success.
pub fn describe_ui<H: PluginHost + 'static>(host: H, manifest: &Manifest, component: &[u8]) -> (H, Result<craft_plugin::ui::Declarations>) {
    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    {
        native::ui::describe(host, manifest, component, RuntimeOptions::default())
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        let _ = (manifest, component);
        (host, Err(craft_plugin::HostError::Trap { reason: "component execution requires an x86-64 or AArch64 native build".into() }))
    }
}

/// Deliver a validated panel action and execute its declared entry in one ordinary command transaction.
/// `params` obeys the entry's usual command schema; the typed event is delivered separately.
pub fn run_ui_event<H: PluginHost + 'static>(
    host: H,
    manifest: &Manifest,
    component: &[u8],
    entry: &str,
    params: &Value,
    event: &craft_plugin::ui::UiEvent,
) -> (H, Result<Value>) {
    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    {
        native::run_call(host, manifest, component, entry, params, Some(event), RuntimeOptions::default())
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        let _ = (manifest, component, entry, params, event);
        (host, Err(craft_plugin::HostError::Trap { reason: "component execution requires an x86-64 or AArch64 native build".into() }))
    }
}

/// Runs an export slot on the app's open design in a fresh instance and returns the file's bytes.
/// The plugin reads the design through the read-only commands its manifest lists, in a
/// transaction that is always rolled back, and reaches no file: the app writes the bytes to the
/// path the user chose, and only when the whole call succeeds. A trap fails this call alone.
pub fn run_export<H: PluginHost + 'static>(host: H, manifest: &Manifest, component: &[u8], slot: &str, options: &Value) -> (H, Result<Vec<u8>>) {
    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    {
        native::run_export(host, manifest, component, slot, options, RuntimeOptions::default())
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        let _ = (manifest, component, slot, options);
        (host, Err(craft_plugin::HostError::Trap { reason: "component execution requires an x86-64 or AArch64 native build".into() }))
    }
}
