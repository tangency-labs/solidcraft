//! What a plugin's failure tells the user: the plugin's name first, the file it was opening, and
//! for a limit the numbers and what to change. The text comes from the message catalog
//! (`plugin_error.*`) in the window's language; without catalogs (the CLI, MCP) it is the English
//! here, which a test keeps equal to the catalog's.
use craft_plugin::HostError;

/// The `plugin_error.*` messages and their English text.
pub const MESSAGES: &[(&str, &str)] = &[
    (
        "plugin_error.deadline_stopped",
        "This plugin used all {allowed} ms of guest time allowed.{request} Raise Preferences › Plugins › Max plugin guest time (ms).",
    ),
    (
        "plugin_error.fuel_stopped",
        "This plugin used all {allowed} fuel allowed.{request} Raise Preferences › Plugins › Max plugin fuel, or approve its request when you install it.",
    ),
    ("plugin_error.limit_request", " It asks for {requested}."),
    (
        "plugin_error.memory_stopped",
        "This plugin needed more than the {allowed} MiB of memory allowed.{request} Raise Preferences › Plugins › Max plugin memory (MiB), or approve its request when you install it.",
    ),
    ("plugin_error.design_too_large", "the design it made is larger than {allowed} MiB, the most a plugin can hand over."),
    ("plugin_error.failed", "{plugin}: {reason}"),
    ("plugin_error.failed_export", "{plugin} could not export {file}: {reason}"),
    ("plugin_error.failed_file", "{plugin} could not open {file}: {reason}"),
    ("plugin_error.folder_option_read_approval", "the option {option} needs read approval for its folder"),
    (
        "plugin_error.fuel_ceiling",
        "it asks for {requested} fuel, and plugins may use {allowed}. Install the plugin again and allow its fuel request, or raise Max plugin fuel in Preferences › Plugins (in solidcraft-cli, max_fuel in the --plugin-limits file).",
    ),
    (
        "plugin_error.guest_time_exhausted",
        "it used all {seconds} s of guest time it asks for and was stopped. Ask its author to raise runtime.limits.deadline_ms; if needed, raise Max plugin guest time (ms) in Preferences › Plugins (in solidcraft-cli, max_deadline_ms in the --plugin-limits file).",
    ),
    (
        "plugin_error.host_calls_ceiling",
        "it asks for {requested} host calls, and plugins may use {allowed}. Raise Max plugin host calls in Preferences › Plugins (in solidcraft-cli, max_host_calls in the --plugin-limits file).",
    ),
    (
        "plugin_error.memory_ceiling",
        "it asks for {requested} MiB of memory, and plugins may use {allowed} MiB. Install the plugin again and allow its memory request, or raise Max plugin memory (MiB) in Preferences › Plugins (in solidcraft-cli, max_memory_mib in the --plugin-limits file).",
    ),
    ("plugin_error.out_of_fuel", "it used all {allowed} fuel it asks for and was stopped. Ask its author to raise its fuel limit."),
    (
        "plugin_error.out_of_host_calls",
        "it used all {allowed} host calls it asks for and was stopped. Ask its author to raise runtime.limits.host_calls; if needed, raise Max plugin host calls in Preferences › Plugins (in solidcraft-cli, max_host_calls in the --plugin-limits file).",
    ),
    (
        "plugin_error.out_of_memory",
        "it needed more than the {allowed} MiB of memory it asks for and was stopped. Ask its author to raise its memory limit.",
    ),
    ("plugin_error.out_of_stack", "it ran out of its {allowed} KiB stack and was stopped. Ask its author to raise its stack size."),
    ("plugin_error.out_of_time", "it ran longer than its time limit of {seconds} s and was stopped. Ask its author to raise its time limit."),
    (
        "plugin_error.stale_revision",
        "the design changed since this edit was calculated (expected {expected}; current {actual}). Run the calculation again.",
    ),
    (
        "plugin_error.time_ceiling",
        "it asks for {requested} ms of guest time, and plugins may use {allowed} ms. Raise Max plugin guest time (ms) in Preferences › Plugins (in solidcraft-cli, max_deadline_ms in the --plugin-limits file).",
    ),
    ("plugin_error.trap", "it stopped on an error inside the plugin ({reason}). Report this to its author."),
    ("plugin_error.unsupported", "{plugin} does not support this: {reason}"),
    ("plugin_error.unsupported_file", "{plugin} does not support {file}: {reason}"),
    (
        "plugin_error.user_storage_conflict",
        "its user settings changed or are being written by another run. Retry the plugin run to load the latest settings.",
    ),
];

/// A plugin's failure: which plugin, the file it was opening (an importer), and what went wrong.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginFailure {
    /// The plugin's name, from its manifest.
    pub plugin: String,
    /// The file's name, without its folder.
    pub file: Option<String>,
    /// The file is being exported, rather than opened.
    pub exporting: bool,
    pub error: HostError,
    /// The explicitly declared manifest limits, when present.
    pub requested_limits: Option<serde_json::Value>,
}

/// `id`'s text in this thread's language, or its English when no catalog has it.
pub(super) fn say(id: &str, args: &[(&str, &str)]) -> String {
    let text = solidcraft_ui_text::format(id, args);
    if text != id {
        return text;
    }
    let english = MESSAGES.iter().find(|(known, _)| *known == id).map_or(id, |(_, english)| english);
    fill(english, args)
}

/// Fills the `{name}` placeholders in `template` in one pass, so the text put in for one (a file
/// named `part{reason}.smp`) is never read as another.
fn fill(template: &str, args: &[(&str, &str)]) -> String {
    let mut text = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        let (before, after) = rest.split_at(open);
        text.push_str(before);
        let filled = after.find('}').and_then(|close| {
            let name = after.get(1..close)?;
            args.iter().find(|(known, _)| *known == name).map(|(_, value)| (*value, close))
        });
        match filled {
            Some((value, close)) => {
                text.push_str(value);
                rest = after.get(close + 1..).unwrap_or_default();
            }
            None => {
                text.push('{');
                rest = after.get(1..).unwrap_or_default();
            }
        }
    }
    text.push_str(rest);
    text
}

/// Group large budgets for readability without changing their exact value.
fn grouped(value: u64) -> String {
    let digits = value.to_string();
    let mut result = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            result.push(',');
        }
        result.push(digit);
    }
    result
}

impl PluginFailure {
    /// What went wrong, after the plugin's name.
    fn reason(&self) -> String {
        let number = |n: u64| n.to_string();
        match &self.error {
            HostError::StaleRevision { expected, actual } => {
                let token = |token: &craft_plugin::DesignToken| format!("{}@{}", token.document_id, token.revision);
                say("plugin_error.stale_revision", &[("expected", &token(expected)), ("actual", &token(actual))])
            }
            HostError::OverCeiling { limit, requested, allowed } if limit == "memory_mib" => {
                say("plugin_error.memory_ceiling", &[("requested", &number(*requested)), ("allowed", &number(*allowed))])
            }
            HostError::OverCeiling { limit, requested, allowed } if limit == "fuel" => {
                say("plugin_error.fuel_ceiling", &[("requested", &number(*requested)), ("allowed", &number(*allowed))])
            }
            HostError::OverCeiling { limit, requested, allowed } if limit == "host_calls" => {
                say("plugin_error.host_calls_ceiling", &[("requested", &number(*requested)), ("allowed", &number(*allowed))])
            }
            HostError::OverCeiling { limit, requested, allowed } if limit == "deadline_ms" => {
                say("plugin_error.time_ceiling", &[("requested", &number(*requested)), ("allowed", &number(*allowed))])
            }
            HostError::Exhausted { limit, allowed } => {
                let key = match limit.as_str() {
                    "fuel" => Some("plugin_error.fuel_stopped"),
                    "memory_mib" => Some("plugin_error.memory_stopped"),
                    "deadline_ms" => Some("plugin_error.deadline_stopped"),
                    _ => None,
                };
                if let Some(key) = key {
                    let requested = self.requested_limits.as_ref().and_then(|limits| limits.get(limit)).and_then(serde_json::Value::as_u64);
                    let request = requested.map_or_else(String::new, |value| say("plugin_error.limit_request", &[("requested", &grouped(value))]));
                    say(key, &[("allowed", &grouped(*allowed)), ("request", &request)])
                } else {
                    match limit.as_str() {
                        "host_calls" => say("plugin_error.out_of_host_calls", &[("allowed", &number(*allowed))]),
                        "stack_kib" => say("plugin_error.out_of_stack", &[("allowed", &number(*allowed))]),
                        "design_mib" => say("plugin_error.design_too_large", &[("allowed", &number(*allowed))]),
                        _ => self.error.to_string(),
                    }
                }
            }
            HostError::Trap { reason } => say("plugin_error.trap", &[("reason", reason)]),
            HostError::UserStorageConflict { .. } => say("plugin_error.user_storage_conflict", &[]),
            // What the plugin or the host said about the input, without the field's name.
            HostError::InvalidInput { reason, .. } | HostError::Unsupported { reason } => reason.clone(),
            error => error.to_string(),
        }
    }

    /// The whole message, in this thread's language.
    pub fn text(&self) -> String {
        let reason = self.reason();
        let unsupported = matches!(self.error, HostError::Unsupported { .. });
        match (&self.file, unsupported) {
            (Some(file), true) => say("plugin_error.unsupported_file", &[("plugin", &self.plugin), ("file", file), ("reason", &reason)]),
            (Some(file), false) => say(
                if self.exporting { "plugin_error.failed_export" } else { "plugin_error.failed_file" },
                &[("plugin", &self.plugin), ("file", file), ("reason", &reason)],
            ),
            (None, true) => say("plugin_error.unsupported", &[("plugin", &self.plugin), ("reason", &reason)]),
            (None, false) => say("plugin_error.failed", &[("plugin", &self.plugin), ("reason", &reason)]),
        }
    }
}

/// Read explicit limit requests using the installed package's existing authority.
pub(super) fn format_limits(session: &super::Session, folder: &str) -> Option<serde_json::Value> {
    super::with_installed(session, folder, || {
        let path = std::path::Path::new(folder).join("plugin.json");
        let bytes = solidcraft_io::vfs::read(&path.to_string_lossy(), craft_plugin::MAX_MANIFEST_BYTES).ok()?;
        serde_json::from_slice::<serde_json::Value>(&bytes).ok()?.pointer("/runtime/limits").cloned()
    })
}

/// Names the plugin and retains its explicit resource requests.
pub(crate) fn named_with_limits(
    plugin: &str,
    file: Option<&str>,
    exporting: bool,
    requested_limits: Option<serde_json::Value>,
) -> impl Fn(crate::EngineError) -> crate::EngineError {
    named_for(plugin, file, exporting, requested_limits)
}

fn named_for(
    plugin: &str,
    file: Option<&str>,
    exporting: bool,
    requested_limits: Option<serde_json::Value>,
) -> impl Fn(crate::EngineError) -> crate::EngineError {
    move |error| match error {
        crate::EngineError::Plugin(HostError::Cancelled) => crate::EngineError::Plugin(HostError::Cancelled),
        crate::EngineError::Plugin(error) => crate::EngineError::PluginFailed(Box::new(PluginFailure {
            plugin: plugin.into(),
            file: file.map(String::from),
            exporting,
            error,
            requested_limits: requested_limits.clone(),
        })),
        e @ (crate::EngineError::Other(_) | crate::EngineError::Feature(_)) => {
            let text = e.to_string();
            let text = text.strip_prefix("plugin: ").unwrap_or(&text);
            match file {
                Some(file) => crate::EngineError::Other(say(
                    if exporting { "plugin_error.failed_export" } else { "plugin_error.failed_file" },
                    &[("plugin", plugin), ("file", file), ("reason", text)],
                )),
                None => crate::EngineError::Other(say("plugin_error.failed", &[("plugin", plugin), ("reason", text)])),
            }
        }
        error => error,
    }
}
