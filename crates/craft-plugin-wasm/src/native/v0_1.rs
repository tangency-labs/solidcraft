//! Plugins built against `craft:plugin@0.1.0` (the frozen `craft-plugin/wit-0.1`), run unchanged.
//! 0.1 has no `plugin-error.unsupported`; every host call here is the current one, with its
//! types converted, so the two versions can't drift apart in what they allow.
use super::{State, control, design, evaluate, files, host_error, log, source};
use craft_plugin::{HostError, PluginHost};
use wasmtime::component::{Component, HasSelf, Linker};
use wasmtime::{Engine, Store};

mod bindings {
    wasmtime::component::bindgen!({ path: "../craft-plugin/wit-0.1", world: "command-plugin" });
}
include!(concat!(env!("OUT_DIR"), "/dialog_0_1.rs"));
mod importer_bindings {
    wasmtime::component::bindgen!({
        path: "../craft-plugin/wit-0.1",
        world: "importer-plugin",
        with: {
            "craft:plugin/types": super::bindings::craft::plugin::types,
            "craft:plugin/log": super::bindings::craft::plugin::log,
            "craft:plugin/control": super::bindings::craft::plugin::control,
            "craft:plugin/clock": super::bindings::craft::plugin::clock,
        },
    });
}
use bindings::craft::plugin as old;
use importer_bindings::craft::plugin as old_formats;
type OldError = old::types::PluginError;

/// A current error, as a 0.1 guest can read it: `unsupported` was `invalid-input` then.
fn older(error: super::PluginError) -> OldError {
    use super::PluginError as New;
    match error {
        New::PermissionDenied(permission) => OldError::PermissionDenied(permission),
        New::CommandFailed(super::types::CommandFailure { command, reason }) => {
            OldError::CommandFailed(old::types::CommandFailure { command, reason })
        }
        New::LimitExceeded(limit) => OldError::LimitExceeded(limit),
        New::InvalidManifest(super::types::FieldFailure { field, reason }) => OldError::InvalidManifest(old::types::FieldFailure { field, reason }),
        New::InvalidInput(super::types::FieldFailure { field, reason }) => OldError::InvalidInput(old::types::FieldFailure { field, reason }),
        New::Unsupported(reason) => OldError::InvalidInput(old::types::FieldFailure { field: "input".into(), reason }),
        New::Trap(reason) => OldError::Trap(reason),
        New::Cancelled => OldError::Cancelled,
    }
}

/// A 0.1 guest's error, as the host reports it.
fn newer(error: OldError) -> HostError {
    use super::types::{CommandFailure, FieldFailure};
    host_error(match error {
        OldError::PermissionDenied(permission) => super::PluginError::PermissionDenied(permission),
        OldError::CommandFailed(old::types::CommandFailure { command, reason }) => {
            super::PluginError::CommandFailed(CommandFailure { command, reason })
        }
        OldError::LimitExceeded(limit) => super::PluginError::LimitExceeded(limit),
        OldError::InvalidManifest(old::types::FieldFailure { field, reason }) => super::PluginError::InvalidManifest(FieldFailure { field, reason }),
        OldError::InvalidInput(old::types::FieldFailure { field, reason }) => super::PluginError::InvalidInput(FieldFailure { field, reason }),
        OldError::Trap(reason) => super::PluginError::Trap(reason),
        OldError::Cancelled => super::PluginError::Cancelled,
    })
}

impl<H: PluginHost> old::types::Host for State<H> {}
impl<H: PluginHost> old::commands::Host for State<H> {
    fn run(&mut self, command: String, args: String) -> Result<String, OldError> {
        super::commands::Host::run(self, command, args).map_err(older)
    }
}
impl<H: PluginHost> old::log::Host for State<H> {
    fn log(&mut self, level: old::log::Level, message: String) {
        let level = match level {
            old::log::Level::Debug => log::Level::Debug,
            old::log::Level::Info => log::Level::Info,
            old::log::Level::Warn => log::Level::Warn,
            old::log::Level::Error => log::Level::Error,
        };
        log::Host::log(self, level, message);
    }
}
impl<H: PluginHost> old::control::Host for State<H> {
    fn progress(&mut self, fraction: f32, message: String) {
        control::Host::progress(self, fraction, message);
    }
    fn cancelled(&mut self) -> bool {
        control::Host::cancelled(self)
    }
}
impl<H: PluginHost> old::clock::Host for State<H> {
    fn now(&mut self) -> u64 {
        super::clock::Host::now(self)
    }
}
impl<H: PluginHost> old::files::Host for State<H> {
    fn read_file(&mut self, grant: String, path: String) -> Result<Vec<u8>, OldError> {
        files::Host::read_file(self, grant, path).map_err(older)
    }
    fn write_file(&mut self, grant: String, path: String, data: Vec<u8>) -> Result<(), OldError> {
        files::Host::write_file(self, grant, path, data).map_err(older)
    }
}
impl<H: PluginHost> old_formats::source::Host for State<H> {
    fn read(&mut self, path: String, offset: u64, len: u32) -> Result<Vec<u8>, OldError> {
        source::Host::read(self, path, offset, len).map_err(older)
    }
    fn size(&mut self, path: String) -> Result<Option<u64>, OldError> {
        source::Host::size(self, path).map_err(older)
    }
    fn resolve(&mut self, path: String) -> Result<Option<String>, OldError> {
        source::Host::resolve(self, path).map_err(older)
    }
}
impl<H: PluginHost> old_formats::design::Host for State<H> {
    fn write(&mut self, bytes: Vec<u8>) -> Result<(), OldError> {
        design::Host::write(self, bytes).map_err(older)
    }
}
impl<H: PluginHost> old_formats::evaluate::Host for State<H> {
    fn stage(&mut self, bytes: Vec<u8>) -> Result<(), OldError> {
        evaluate::Host::stage(self, bytes).map_err(older)
    }
    fn run(&mut self, command: String, args: String) -> Result<String, OldError> {
        evaluate::Host::run(self, command, args).map_err(older)
    }
    fn reset(&mut self) {
        evaluate::Host::reset(self);
    }
}

/// Links a 0.1 component against its world without instantiating it (`plugin check`).
pub(super) fn check_link<H: PluginHost + 'static>(
    engine: &Engine,
    component: &Component,
    importer: bool,
    dialog: bool,
    granted: &[String],
) -> wasmtime::Result<()> {
    let mut linker = Linker::<State<H>>::new(engine);
    super::link_host_query(&mut linker, granted)?;
    if dialog {
        if importer {
            importer_bindings::ImporterPlugin::add_to_linker::<_, HasSelf<_>>(&mut linker, |s| s)?;
        } else {
            dialog_bindings::DialogPlugin::add_to_linker::<_, HasSelf<_>>(&mut linker, |s| s)?;
        }
        dialog_bindings::DialogPluginPre::new(linker.instantiate_pre(component)?)?;
    }
    let mut linker = Linker::<State<H>>::new(engine);
    super::link_host_query(&mut linker, granted)?;
    if importer {
        importer_bindings::ImporterPlugin::add_to_linker::<_, HasSelf<_>>(&mut linker, |s| s)?;
        importer_bindings::ImporterPluginPre::new(linker.instantiate_pre(component)?)?;
    } else {
        bindings::CommandPlugin::add_to_linker::<_, HasSelf<_>>(&mut linker, |s| s)?;
        bindings::CommandPluginPre::new(linker.instantiate_pre(component)?)?;
    }
    Ok(())
}

/// Instantiates a 0.1 command plugin and calls `entry`.
pub(super) fn call_entry<H: PluginHost + 'static>(
    engine: &Engine,
    store: &mut Store<State<H>>,
    component: &Component,
    entry: &str,
    params: &str,
) -> wasmtime::Result<Result<String, HostError>> {
    let mut linker = Linker::new(engine);
    super::link_host_query(&mut linker, &store.data().granted_capabilities)?;
    bindings::CommandPlugin::add_to_linker::<_, HasSelf<_>>(&mut linker, |s| s)?;
    let guest = bindings::CommandPlugin::instantiate(&mut *store, component, &linker)?;
    Ok(guest.craft_plugin_entry().call_execute(&mut *store, entry, &params.to_owned())?.map_err(newer))
}

/// Calls the additive 0.1 hook without changing the legacy command world.
pub(super) fn call_dialog<H: PluginHost + 'static>(
    engine: &Engine,
    store: &mut Store<State<H>>,
    component: &Component,
    entry: &str,
    values: &str,
    importer: bool,
) -> wasmtime::Result<Result<String, HostError>> {
    let mut linker = Linker::new(engine);
    super::link_host_query(&mut linker, &store.data().granted_capabilities)?;
    if importer {
        importer_bindings::ImporterPlugin::add_to_linker::<_, HasSelf<_>>(&mut linker, |s| s)?;
    } else {
        dialog_bindings::DialogPlugin::add_to_linker::<_, HasSelf<_>>(&mut linker, |s| s)?;
    }
    let guest = dialog_bindings::DialogPlugin::instantiate(&mut *store, component, &linker)?;
    Ok(guest.craft_plugin_dialog_provider().call_update_fields(&mut *store, entry, &values.to_owned())?.map_err(newer))
}

/// Instantiates a 0.1 importer and converts `file`.
pub(super) fn call_import<H: PluginHost + 'static>(
    engine: &Engine,
    store: &mut Store<State<H>>,
    component: &Component,
    entry: &str,
    file: &super::source::FileInfo,
    options: &str,
) -> wasmtime::Result<Result<String, HostError>> {
    let mut linker = Linker::new(engine);
    super::link_host_query(&mut linker, &store.data().granted_capabilities)?;
    importer_bindings::ImporterPlugin::add_to_linker::<_, HasSelf<_>>(&mut linker, |s| s)?;
    let guest = importer_bindings::ImporterPlugin::instantiate(&mut *store, component, &linker)?;
    let file = old_formats::source::FileInfo { name: file.name.clone(), size: file.size };
    Ok(guest.craft_plugin_importer().call_import_file(&mut *store, entry, &file, &options.to_owned())?.map_err(newer))
}

#[cfg(test)]
#[test]
fn an_unsupported_error_reads_as_invalid_input_to_a_0_1_guest() {
    let older = older(super::PluginError::Unsupported("no solids".into()));
    assert!(matches!(older, OldError::InvalidInput(old::types::FieldFailure { field, reason }) if field == "input" && reason == "no solids"));
}
