//! Whole-design invalidation on the command worker. Never part of the triggering transaction.
use super::*;

/// Deliver one already coalesced invalidation. The window calls this on its command worker,
/// after the host action finishes. No guest result is ever adopted into the design.
pub fn design_changed(session: &mut Session, design: Option<DesignToken>) {
    session.plugin_state.refresh_stored();
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
        let listeners = runtime.listeners.clone();
        let mut failure = None;
        for listener in listeners {
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
            let event = json!({"design":design});
            let invocation = Package { manifest, approval: package.approval.clone(), component: package.component.clone() };
            match run(&mut scratch, invocation, &listener.entry, &event) {
                Ok(reply) => {
                    if let Err(error) = listener_panel_data(session, &id, &listener.entry, reply.result) {
                        failure = Some(error.to_string());
                    }
                }
                Err(error) => failure = Some(error.to_string()),
            }
        }
        match failure {
            Some(error) => {
                retain_failure(session, id, error);
            }
            None => {
                session.plugin_state.listener_failures.remove(&id);
            }
        }
    }
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
