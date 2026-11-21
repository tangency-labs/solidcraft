//! Installed plugins as tools for agents (RFC 0002 section 6c): every enabled plugin's entries,
//! listed with their declared inputs (`plugin.tools`), and the refusals a tool call can meet put
//! in words an agent can act on: which plugin, which grant and how to approve it, or which host
//! import a `read` entry was refused. A refusal is an error, never a prompt.
use super::*;
use crate::schema::plugin::{PluginTool, PluginToolKind};

/// How a person approves a grant a plugin lacks.
pub const APPROVE: &str = "approve it in Preferences › Plugins by installing the plugin again and allowing it, \
or headlessly with plugin.install and its approve object (kept for later sessions; plugin.grants lists what is approved)";

/// Every installed, enabled plugin's entries as tools, sorted by name, or only the tool `name`.
/// Reads each manifest, and for the one tool `name` asks for its component (for the approval
/// hash its call passes); runs no plugin code. A disabled or removed plugin has no tools.
pub fn tools(session: &mut Session, name: Option<&str>) -> Vec<PluginTool> {
    let mut found = Vec::new();
    let wanted = name.map(|n| n.split_once('.').map_or(n, |(plugin, _)| plugin));
    for (id, folder) in import::package_folders(session) {
        if session.disabled_plugins.contains(&id) || wanted.is_some_and(|w| w != id) {
            continue;
        }
        let Ok(manifest) = with_installed(session, &folder, || import::read_manifest(&folder)) else { continue };
        let Some(runtime) = manifest.runtime.as_ref() else { continue };
        // A command entry runs through `<plugin>.<entry>`, which needs the package's hash.
        let approval = if runtime.is_importer() || name.is_none() {
            None
        } else {
            let Ok(package) = with_installed(session, &folder, || read_package(&folder)) else { continue };
            Some(package.approval)
        };
        #[cfg(not(target_arch = "wasm32"))]
        session.plugin_state.register_discovered(id.clone(), folder.clone());
        for entry in &runtime.entries {
            let (kind, input_schema) = match entry.kind {
                EntryKind::Command => (PluginToolKind::Command, tool_input(entry)),
                EntryKind::Importer => (PluginToolKind::Importer, importer_schema(entry)),
            };
            found.push(PluginTool {
                name: format!("{}.{}", manifest.id, entry.name),
                kind,
                plugin: manifest.id.clone(),
                entry: entry.name.clone(),
                plugin_name: manifest.name.clone(),
                plugin_version: manifest.version.clone(),
                description: manifest.description.clone(),
                effect: entry.effect,
                extensions: entry.extensions.clone(),
                input_schema,
                approval: (kind == PluginToolKind::Command).then(|| approval.clone()).flatten(),
            });
        }
    }
    found.retain(|t| name.is_none_or(|n| t.name == n));
    found.sort_by(|a, b| a.name.cmp(&b.name));
    found
}

/// See [`super::describe_entry`].
pub(super) fn describe(session: &Session, id: &str) -> Result<Option<Value>> {
    if crate::find_command(id).is_some() {
        return Ok(None);
    }
    let Some((plugin, name)) = id.split_once('.') else { return Ok(None) };
    if !valid_id(plugin) || !valid_id(name) || session.disabled_plugins.contains(plugin) {
        return Ok(None);
    }
    let folders = import::package_folders(session);
    let Some(folder) = folders.get(plugin) else { return Ok(None) };
    let manifest = with_installed(session, folder, || import::read_manifest(folder))?;
    let Some(runtime) = manifest.runtime else { return Ok(None) };
    let Some(entry) = runtime.entries.iter().find(|e| e.name == name) else { return Ok(None) };
    Ok(Some(match entry.kind {
        EntryKind::Command => {
            let inputs = tool_input(entry);
            json!({"id":id,"label":name,"kind":"command","plugin":plugin,
                "params":"approval: the package hash (plugin.tools {name} or plugin.inspect), params: the entry's declared inputs, expected_design?: token this edit was computed against; as a tool, the inputs alone",
                "input":{"type":"object","required":["approval","params"],"additionalProperties":false,"properties":{
                    "approval":{"type":"string"},"params":inputs.clone(),
                    "expected_design":schemars::schema_for!(craft_plugin::DesignToken).to_value()}},
                "tool":{"name":id,"input":inputs},
                "output":{},"effect":entry.effect,"slots":entry.slots})
        }
        EntryKind::Importer => {
            let tool = importer_schema(entry);
            json!({"id":id,"label":name,"kind":"importer","plugin":plugin,"extensions":entry.extensions,
                "params":format!("an importer, not a command: open a file with file.import {{path, format: \"{id}\", options}} or doc.open {{path}}, or call the tool {id} {{path, options}}"),
                "input":tool.clone(),
                "tool":{"name":id,"input":tool},
                "output":{},"effect":entry.effect,"slots":entry.slots})
        }
    }))
}

/// Hook defaults are resolved by the engine, so the transport accepts omitted keys. All other
/// constraints remain; the original required list is checked after the single hook call.
fn tool_input(entry: &Entry) -> Value {
    let mut schema = craft_plugin::strict_input_schema(&entry.input_schema);
    if entry.dialog_hook
        && let Some(object) = schema.as_object_mut()
    {
        object.remove("required");
    }
    schema
}

/// An importer tool's arguments: the file, and the options the importer declares.
fn importer_schema(entry: &Entry) -> Value {
    let extensions: Vec<String> = entry.extensions.iter().map(|e| format!(".{e}")).collect();
    json!({
        "type": "object",
        "properties": {
            "path": {"type": "string", "description": format!("The file to open ({})", extensions.join(", "))},
            "options": tool_input(entry),
        },
        "required": ["path"],
        "additionalProperties": false,
    })
}

/// The refusal of a call that needs a grant `plugin` does not have.
pub fn missing_grant(plugin: &str, grant: &str) -> EngineError {
    EngineError::Other(format!("plugin `{plugin}` does not have the grant `{grant}`: {APPROVE}"))
}

/// The refusal of something a `read` entry may not do: a host import (`write-file`) or an edit
/// command, named in `what` ("import `write-file`", "command `solid.extrude`").
fn read_only(plugin: &str, entry: &str, kind: &str, name: &str) -> EngineError {
    EngineError::Other(format!("plugin `{plugin}` entry `{entry}` is a read entry and was refused the {kind} `{name}`: it may not change anything"))
}

/// A failed `<plugin>.<entry>` call's error in words an agent can act on ([`explain`]), with the
/// grants `session` holds for the plugin.
pub fn explain_named(session: &Session, manifest: &Manifest, entry: &str, error: EngineError) -> EngineError {
    let grants = session.plugin_state.grants.get(&manifest.id).cloned().unwrap_or_default();
    match error {
        EngineError::Plugin(e) => explain(e, manifest, &grants, entry),
        EngineError::PluginFailed(failure) => {
            // Keep the catalog's plugin, file and budget details beside the tool's structured refusal.
            match explain(failure.error.clone(), manifest, &grants, entry) {
                EngineError::Other(explanation) => EngineError::Other(format!("{} {explanation}", failure.text())),
                _ => EngineError::PluginFailed(failure),
            }
        }
        e => e,
    }
}

/// A failed importer tool call's error in words an agent can act on, as [`explain_named`].
pub fn explain_importer(session: &Session, importer: &import::Importer, error: EngineError) -> EngineError {
    match with_installed(session, &importer.folder, || import::read_manifest(&importer.folder)) {
        Ok(manifest) => explain_named(session, &manifest, &importer.entry, error),
        Err(_) => error,
    }
}

/// A run's failure in words an agent can act on: a grant the plugin lacks (named, with how to
/// approve it) or what a read entry was refused (named). Anything else is left as it was.
fn explain(error: HostError, manifest: &Manifest, grants: &Grants, entry: &str) -> EngineError {
    let id = &manifest.id;
    let Some(runtime) = manifest.runtime.as_ref() else { return error.into() };
    match &error {
        HostError::PermissionDenied { permission } => {
            if let Some(import) = permission.strip_prefix(craft_plugin::READ_ENTRY_IMPORT) {
                return read_only(id, entry, "host import", import);
            }
            if let Some(command) = permission.strip_prefix("edit command ").and_then(|p| p.strip_suffix(" from read entry")) {
                return read_only(id, entry, "edit command", command);
            }
            // `files.read:<name>` or `files.write:<name>`, with nothing after: no such grant.
            for (access, write) in [("files.read:", false), ("files.write:", true)] {
                if let Some(name) = permission.strip_prefix(access)
                    && !name.contains(':')
                    && runtime.permissions.files.iter().any(|f| f.name == name && if write { f.write } else { f.read })
                    && !grants.folders.iter().any(|g| g.name == name && if write { g.write } else { g.read })
                {
                    return missing_grant(id, &format!("{access}{name}"));
                }
            }
            if let Some(group) = permission.split('`').nth(1).filter(|_| permission.contains(" permission"))
                && runtime.permissions.command_groups.contains(group)
                && !grants.command_groups.contains(group)
            {
                return missing_grant(id, &format!("command_groups.{group}"));
            }
            if permission.starts_with("stored plugin grants need re-approval") {
                return missing_grant(id, "approval");
            }
            error.into()
        }
        HostError::LimitExceeded { limit } if limit.starts_with("memory exceeds") => missing_grant(id, "memory_mib"),
        HostError::OverCeiling { limit, .. } if limit == "memory_mib" => missing_grant(id, "memory_mib"),
        _ => error.into(),
    }
}

/// A plugin tool refusal as data, from its text (wherever it sits in a longer message):
/// `{plugin, grant, approve}` for a missing grant, `{plugin, entry, refused_import}` or
/// `{plugin, entry, refused_command}` for a read entry. `None` for any other text.
pub fn structured_error(text: &str) -> Option<Value> {
    if text.len() > 8192 {
        return None;
    }
    if let Some((_, rest)) = text.split_once("interactive_required: opening ")
        && let Some((entry, _)) = rest.split_once(" requires the app form;")
        && entry.split('.').all(valid_id)
    {
        return Some(json!({"kind":"interactive_required","entry":entry,"action":"call_entry_with_params"}));
    }
    let start = text.find("plugin `")?;
    let rest = &text[start + "plugin `".len()..];
    let (plugin, rest) = rest.split_once('`')?;
    if !valid_id(plugin) {
        return None;
    }
    if let Some(rest) = rest.strip_prefix(" does not have the grant `") {
        let (grant, rest) = rest.split_once('`')?;
        rest.starts_with(": ").then(|| json!({"plugin": plugin, "grant": grant, "approve": APPROVE}))
    } else {
        let rest = rest.strip_prefix(" entry `")?;
        let (entry, rest) = rest.split_once('`')?;
        let rest = rest.strip_prefix(" is a read entry and was refused the ")?;
        let (key, rest) = if let Some(r) = rest.strip_prefix("host import `") {
            ("refused_import", r)
        } else {
            ("refused_command", rest.strip_prefix("edit command `")?)
        };
        let (name, _) = rest.split_once('`')?;
        let mut refusal = json!({"plugin": plugin, "entry": entry});
        refusal[key] = json!(name);
        Some(refusal)
    }
}
