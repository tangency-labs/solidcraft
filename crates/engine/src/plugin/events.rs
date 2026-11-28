//! Design and linked-file invalidations on the command worker.
use super::*;

/// The latest observed state of one changed linked source. The digest is host-only: repeated
/// checks of the same bytes do not enqueue another notification.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct LinkChange {
    pub component: u64,
    pub name: Option<String>,
    /// Absolute source path, as held by the open session.
    pub path: String,
    /// Whether the design has read these bytes, rather than only observed them on disk.
    pub read: bool,
    #[serde(skip)]
    pub(crate) digest: String,
}

pub(crate) fn record_link_change(session: &mut Session, component: u64, path: String, digest: String, read: bool) {
    let change =
        LinkChange { component, name: session.doc.components.iter().find(|c| c.id == component).map(|c| c.name.clone()), path, read, digest };
    if session.plugin_state.link_changes.get(&component) != Some(&change) {
        let changes = std::sync::Arc::make_mut(&mut session.plugin_state.link_changes);
        changes.retain(|component, _| session.doc.links.contains_key(component));
        changes.insert(component, change);
    }
}

/// Record only successful source reads. No filesystem access is added to command completion.
pub(crate) fn record_changed_link_reads(session: &mut Session, before: &solidcraft_doc::Document) {
    let reads: Vec<_> = session
        .doc
        .links
        .iter()
        .filter_map(|(component, link)| {
            let previous = before.links.get(component);
            if previous.is_some_and(|old| old.path == link.path && old.file_sha256 == link.file_sha256) {
                return None;
            }
            Some((*component, link.path.clone(), link.file_sha256.clone()?))
        })
        .collect();
    for (component, path, digest) in reads {
        std::sync::Arc::make_mut(&mut session.plugin_state.link_reads).insert(component);
        record_link_change(session, component, path, digest, true);
    }
}

/// Record the selected components whose source was successfully applied, even if its bytes
/// already match the document. Failed or unselected links never enter this list.
pub(crate) fn record_link_reads(session: &mut Session, components: &[u64]) {
    for component in components {
        let Some((path, digest)) = session.doc.links.get(component).and_then(|link| Some((link.path.clone(), link.file_sha256.clone()?))) else {
            continue;
        };
        std::sync::Arc::make_mut(&mut session.plugin_state.link_reads).insert(*component);
        record_link_change(session, *component, path, digest, true);
    }
}

/// Deliver one already coalesced invalidation. The window calls this on its command worker,
/// after the host action finishes. No guest result is ever adopted into the design.
pub fn design_changed(session: &mut Session, design: Option<DesignToken>) {
    design_notifications(session, design, true, &[]);
}

/// Deliver only the events each approved read listener declared. Disk observations alone do
/// not change the design token or invoke existing whole-design listeners.
pub fn design_notifications(session: &mut Session, design: Option<DesignToken>, changed: bool, links: &[LinkChange]) {
    session.plugin_state.refresh_stored();
    let mut approved_listeners = BTreeSet::new();
    for (id, folder) in import::package_folders(session) {
        if session.disabled_plugins.contains(&id) {
            continue;
        }
        let manifest = with_installed(session, &folder, || import::read_manifest(&folder));
        let Ok(manifest) = manifest else { continue };
        if !manifest.runtime.as_ref().is_some_and(|runtime| !runtime.listeners.is_empty()) {
            continue;
        }
        let package = with_installed(session, &folder, || read_package(&folder));
        let package = match package {
            Ok(package) => package,
            Err(error) => {
                retain_failure(session, id, error);
                continue;
            }
        };
        // Background execution requires approval of this exact declaration and component.
        if session.plugin_state.grants.get(&id).and_then(|grants| grants.package_hash.as_deref()) != Some(package.approval.as_str()) {
            continue;
        }
        // Only the declaration included in the approved bytes authorizes invocations.
        let Some(runtime) = package.manifest.runtime.as_ref() else { continue };
        let granted = session.plugin_state.grants.get(&id).map(|grants| grants.folders.as_slice()).unwrap_or_default();
        // Hidden names inside a granted folder stay out of reach, as they do for the package's reads.
        let scope = solidcraft_io::vfs::PathScope::new(approved_file_folders(granted, &runtime.permissions.files, false)).without_hidden();
        // Unread disk state has the same folder boundary as the package's file reads.
        // Versions already read into the design remain visible without a file grant.
        // An unread source must lie inside both the plugin's approved read folders and the
        // session's own scope (`--allow-path`), as `granted_folders` narrows file grants.
        let session_scope = solidcraft_io::vfs::current_scope();
        let links: Vec<_> = links
            .iter()
            .filter(|link| link.read || (scope.check(&link.path).is_ok() && session_scope.as_ref().is_none_or(|s| s.check(&link.path).is_ok())))
            .collect();
        let listeners = runtime.listeners.clone();
        approved_listeners.extend(listeners.iter().map(|listener| format!("{id}.{}", listener.entry)));
        if let Some(failures) = session.plugin_state.listener_entry_failures.get_mut(&id) {
            failures.retain(|entry, _| listeners.iter().any(|listener| listener.entry == *entry));
        }
        let mut invoked = false;
        for listener in listeners {
            let include_links = !links.is_empty() && listener.on.iter().any(|on| on == "design.links_changed");
            if !(include_links || changed && listener.on.iter().any(|on| on == "design.changed")) {
                continue;
            }
            invoked = true;
            std::sync::Arc::make_mut(&mut session.plugin_state.listener_data).remove(&format!("{id}.{}", listener.entry));
            let mut scratch = session.scratch();
            scratch.plugin_state.listener_data = Default::default();
            scratch.plugin_state.listener_design = Some(design.clone());
            let mut manifest = package.manifest.clone();
            let Some(runtime) = manifest.runtime.as_mut() else { continue };
            // These independent budgets never borrow time/fuel from the action that notified us.
            runtime.limits.fuel = runtime.limits.fuel.min(1_000_000);
            runtime.limits.memory_mib = runtime.limits.memory_mib.min(16);
            runtime.limits.deadline_ms = runtime.limits.deadline_ms.min(100);
            runtime.limits.stack_kib = Some(runtime.limits.stack_kib.unwrap_or(512).min(512));
            runtime.permissions.files.clear();
            runtime.permissions.command_groups.clear();
            if let Some(entry) = runtime.entries.iter_mut().find(|entry| entry.name == listener.entry) {
                // The host supplies the event envelope, independently of user entry inputs.
                entry.input_schema = json!({"type":"object"});
            }
            let mut event = json!({"design":design});
            if include_links {
                event["links"] = json!(links);
            }
            let invocation = Package { manifest, approval: package.approval.clone(), component: package.component.clone() };
            let result = run(&mut scratch, invocation, &listener.entry, &event)
                .and_then(|reply| listener_panel_data(session, &id, &listener.entry, reply.result).map_err(EngineError::from));
            match result {
                Ok(()) => {
                    if let Some(failures) = session.plugin_state.listener_entry_failures.get_mut(&id) {
                        failures.remove(&listener.entry);
                    }
                }
                Err(error) => {
                    session
                        .plugin_state
                        .listener_entry_failures
                        .entry(id.clone())
                        .or_default()
                        .insert(listener.entry, error.to_string().chars().take(4096).collect());
                }
            }
        }
        if !invoked {
            continue;
        }
        let failures = session.plugin_state.listener_entry_failures.get(&id);
        if let Some(failures) = failures.filter(|failures| !failures.is_empty()) {
            let diagnostic = failures.iter().map(|(entry, error)| format!("{entry}: {error}")).collect::<Vec<_>>().join("\n");
            retain_failure(session, id, diagnostic);
        } else {
            session.plugin_state.listener_entry_failures.remove(&id);
            session.plugin_state.listener_failures.remove(&id);
        }
    }
    // A partial event keeps outputs of listeners it did not invoke. Replaced packages cannot
    // retain output under an old approval. Disabled packages keep the engine's existing cache
    // behavior; the window filters their output before publishing it.
    std::sync::Arc::make_mut(&mut session.plugin_state.listener_data)
        .retain(|key, _| approved_listeners.contains(key) || key.split_once('.').is_some_and(|(id, _)| session.disabled_plugins.contains(id)));
}

fn retain_failure(session: &mut Session, id: String, error: impl std::fmt::Display) {
    session.plugin_state.listener_failures.insert(id, error.to_string().chars().take(4096).collect());
}

/// Hook for panel support (#19516). Until the panel contract lands, keep only bounded JSON.
pub(super) fn listener_panel_data(session: &mut Session, plugin: &str, entry: &str, data: Value) -> craft_plugin::Result<()> {
    const MAX_RETAINED_BYTES: usize = 16 * 1024 * 1024;
    let json_bytes = serde_json::to_vec(&data).map_err(|error| HostError::Trap { reason: error.to_string() })?.len();
    if json_bytes > MAX_JSON_BYTES {
        return Err(HostError::LimitExceeded { limit: format!("listener reply exceeds {MAX_JSON_BYTES} JSON bytes") });
    }
    let key = format!("{plugin}.{entry}");
    // Measure the prospective map by reference, before publishing anything. This work stays
    // on the command worker; AppState and Session copies share the retained JSON via Arc.
    let mut retained: BTreeMap<&str, &Value> = session.plugin_state.listener_data.iter().map(|(key, value)| (key.as_str(), value)).collect();
    retained.insert(&key, &data);
    if serde_json::to_vec(&retained).map_err(|error| HostError::Trap { reason: error.to_string() })?.len() > MAX_RETAINED_BYTES {
        return Err(HostError::LimitExceeded { limit: format!("listener panel data exceeds {MAX_RETAINED_BYTES} retained JSON bytes") });
    }
    std::sync::Arc::make_mut(&mut session.plugin_state.listener_data).insert(key, data);
    Ok(())
}
