//! What a plugin's failure tells the user: the plugin's name first, the file it was opening, and
//! for a limit the numbers and what to change. The text comes from the message catalog
//! (`plugin_error.*`) in the window's language; without catalogs (the CLI, MCP) it is the English
//! here, which a test keeps equal to the catalog's.
use craft_plugin::HostError;

/// The `plugin_error.*` messages and their English text.
pub const MESSAGES: &[(&str, &str)] = &[
    ("plugin_error.design_too_large", "the design it made is larger than {allowed} MiB, the most a plugin can hand over."),
    ("plugin_error.failed", "{plugin}: {reason}"),
    ("plugin_error.failed_file", "{plugin} could not open {file}: {reason}"),
    (
        "plugin_error.fuel_ceiling",
        "it asks for {requested} fuel, and plugins may use {allowed}. Raise Max plugin fuel in Preferences › Plugins (in solidcraft-cli, max_fuel in the --plugin-limits file).",
    ),
    (
        "plugin_error.memory_ceiling",
        "it asks for {requested} MiB of memory, and plugins may use {allowed} MiB. Install the plugin again and allow its memory request, or raise Max plugin memory (MiB) in Preferences › Plugins (in solidcraft-cli, max_memory_mib in the --plugin-limits file).",
    ),
    ("plugin_error.out_of_fuel", "it used all {allowed} fuel it asks for and was stopped. Ask its author to raise its fuel limit."),
    (
        "plugin_error.out_of_memory",
        "it needed more than the {allowed} MiB of memory it asks for and was stopped. Ask its author to raise its memory limit.",
    ),
    ("plugin_error.out_of_stack", "it ran out of its {allowed} KiB stack and was stopped. Ask its author to raise its stack size."),
    ("plugin_error.out_of_time", "it ran longer than its time limit of {seconds} s and was stopped. Ask its author to raise its time limit."),
    ("plugin_error.trap", "it stopped on an error inside the plugin ({reason}). Report this to its author."),
    ("plugin_error.unsupported", "{plugin} does not support this: {reason}"),
    ("plugin_error.unsupported_file", "{plugin} does not support {file}: {reason}"),
];

/// A plugin's failure: which plugin, the file it was opening (an importer), and what went wrong.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginFailure {
    /// The plugin's name, from its manifest.
    pub plugin: String,
    /// The file's name, without its folder.
    pub file: Option<String>,
    pub error: HostError,
}

/// `id`'s text in this thread's language, or its English when no catalog has it.
fn say(id: &str, args: &[(&str, &str)]) -> String {
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

/// Seconds, as a person reads them: 300, or 0.05.
fn seconds(ms: u64) -> String {
    if ms.is_multiple_of(1000) { (ms / 1000).to_string() } else { format!("{}", ms as f64 / 1000.0) }
}

impl PluginFailure {
    /// What went wrong, after the plugin's name.
    fn reason(&self) -> String {
        let number = |n: u64| n.to_string();
        match &self.error {
            HostError::OverCeiling { limit, requested, allowed } if limit == "memory_mib" => {
                say("plugin_error.memory_ceiling", &[("requested", &number(*requested)), ("allowed", &number(*allowed))])
            }
            HostError::OverCeiling { limit, requested, allowed } if limit == "fuel" => {
                say("plugin_error.fuel_ceiling", &[("requested", &number(*requested)), ("allowed", &number(*allowed))])
            }
            HostError::Exhausted { limit, allowed } => match limit.as_str() {
                "fuel" => say("plugin_error.out_of_fuel", &[("allowed", &number(*allowed))]),
                "memory_mib" => say("plugin_error.out_of_memory", &[("allowed", &number(*allowed))]),
                "stack_kib" => say("plugin_error.out_of_stack", &[("allowed", &number(*allowed))]),
                "deadline_ms" => say("plugin_error.out_of_time", &[("seconds", &seconds(*allowed))]),
                "design_mib" => say("plugin_error.design_too_large", &[("allowed", &number(*allowed))]),
                _ => self.error.to_string(),
            },
            HostError::Trap { reason } => say("plugin_error.trap", &[("reason", reason)]),
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
            (Some(file), false) => say("plugin_error.failed_file", &[("plugin", &self.plugin), ("file", file), ("reason", &reason)]),
            (None, true) => say("plugin_error.unsupported", &[("plugin", &self.plugin), ("reason", &reason)]),
            (None, false) => say("plugin_error.failed", &[("plugin", &self.plugin), ("reason", &reason)]),
        }
    }
}

/// Names `plugin` (and `file`) in a failure of its run. A cancel is not the plugin's failure, and
/// an error that is not the plugin's (a design that failed to rebuild) is kept as it is.
pub(crate) fn named(plugin: &str, file: Option<&str>) -> impl Fn(crate::EngineError) -> crate::EngineError {
    move |error| match error {
        crate::EngineError::Plugin(HostError::Cancelled) => crate::EngineError::Plugin(HostError::Cancelled),
        crate::EngineError::Plugin(error) => {
            crate::EngineError::PluginFailed(Box::new(PluginFailure { plugin: plugin.into(), file: file.map(String::from), error }))
        }
        e @ (crate::EngineError::Other(_) | crate::EngineError::Feature(_)) => {
            let text = e.to_string();
            let text = text.strip_prefix("plugin: ").unwrap_or(&text);
            match file {
                Some(file) => crate::EngineError::Other(say("plugin_error.failed_file", &[("plugin", plugin), ("file", file), ("reason", text)])),
                None => crate::EngineError::Other(say("plugin_error.failed", &[("plugin", plugin), ("reason", text)])),
            }
        }
        error => error,
    }
}
