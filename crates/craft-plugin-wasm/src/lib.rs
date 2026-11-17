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
