//! How a command an agent runs reads to the person watching: its label from the command catalog
//! ("Fillet"), never the id an MCP client or the control channel names it by
//! (`mcp__solidcraft-app__execute`, `mcp.solidcraft_fillet`, `ui.prefs`).
//!
//! [`name`] is the one place that turns such a name into words; the operations panel, the agent's
//! cursor, the status line and the Assistant's tool cards all use it.

use solidcraft_ui_text::Locale;

/// The most commands a batch's name lists before it ends with an ellipsis.
const BATCH_NAMES: usize = 3;

/// Control methods whose name has a message of its own.
const METHOD_MESSAGES: &[(&str, &str)] = &[("ui.prefs", "ui.preferences"), ("ui.screenshot", "ui.agent_tool.screenshot")];

/// The name of an MCP tool or control method without its server: `mcp__solidcraft-app__batch`,
/// `mcp.solidcraft-app.batch`, `mcp:solidcraft/batch` and `mcp.solidcraft_batch` are all `batch`.
/// A name that is no MCP name comes back as it is.
pub fn tool_of(raw: &str) -> &str {
    let raw = raw.trim();
    let Some(rest) = raw.get(..3).filter(|p| p.eq_ignore_ascii_case("mcp")).and_then(|_| raw.get(3..)) else { return raw };
    let Some(rest) = rest.strip_prefix("__").or_else(|| rest.strip_prefix(['.', ':', '/', '_'])) else { return raw };
    let server = rest.split_once("__").or_else(|| rest.split_once(['.', ':', '/']));
    match server {
        Some((_, tool)) if !tool.is_empty() => tool,
        _ => rest.strip_prefix("solidcraft_").or_else(|| rest.strip_prefix("solidcraft-")).filter(|t| !t.is_empty()).unwrap_or(rest),
    }
}

/// Whether `raw` is an MCP tool's id (`mcp__server__tool` and its dotted forms).
pub fn is_mcp_id(raw: &str) -> bool {
    tool_of(raw) != raw.trim()
}

/// A word list from an id: `set_parameter`, `ui.prefs`, `solid-fillet` read "Set parameter",
/// "Prefs", "Solid fillet".
fn humanised(id: &str) -> String {
    let last = id.rsplit(['.', ':', '/']).find(|s| !s.is_empty()).unwrap_or(id);
    let words: Vec<&str> = last.split(['_', '-', ' ']).filter(|w| !w.is_empty()).collect();
    let mut text = words.join(" ");
    if let Some(first) = text.chars().next() {
        let upper: String = first.to_uppercase().collect();
        text.replace_range(..first.len_utf8(), &upper);
    }
    text
}

/// The command a tool's name stands for: the id itself, `solid_fillet` as `solid.fillet`, or a
/// bare `fillet` when exactly one command ends in it.
fn command_of(id: &str) -> Option<&'static solidcraft_engine::CommandSpec> {
    if let Some(c) = solidcraft_engine::find_command(id) {
        return Some(c);
    }
    if !id.contains('.')
        && id.contains('_')
        && let Some(c) = solidcraft_engine::find_command(&id.replacen('_', ".", 1))
    {
        return Some(c);
    }
    if id.contains('.') || id.is_empty() {
        return None;
    }
    let dotted = format!(".{id}");
    let mut found = solidcraft_engine::command_specs().into_iter().filter(|c| c.id.ends_with(&dotted));
    found.next().filter(|_| found.next().is_none())
}

/// What a command, tool or control method is called for a person, in `language`.
pub fn name_in(language: Locale, raw: &str) -> String {
    let id = tool_of(raw);
    if let Some(c) = solidcraft_engine::agent_tools::tool_command(id).and_then(solidcraft_engine::find_command) {
        return language.label(c.label).to_string();
    }
    if let Some(c) = command_of(id) {
        return language.label(c.label).to_string();
    }
    let methods = METHOD_MESSAGES.iter().find(|(m, _)| *m == id).map(|(_, message)| (*message).to_string());
    // The channel's own methods ("engine.batch") read as the tool they are.
    let message = methods.unwrap_or_else(|| format!("ui.agent_tool.{}", id.rsplit('.').next().unwrap_or(id)));
    let text = language.text(&message);
    if text != message {
        return text.to_string();
    }
    humanised(id)
}

/// [`name_in`] in the language being drawn.
pub fn name(raw: &str) -> String {
    name_in(solidcraft_ui_text::locale(), raw)
}

/// A batch's name from its commands' names: "Batch: Fillet, Chamfer, …".
pub fn batch_in(language: Locale, names: &[String]) -> String {
    batch_of(language, names, names.len() > BATCH_NAMES)
}

/// A batch's name from the first of its commands' names, `more` saying that others follow.
fn batch_of(language: Locale, names: &[String], more: bool) -> String {
    let mut list = names.iter().take(BATCH_NAMES).cloned().collect::<Vec<_>>().join(", ");
    if more {
        list.push_str(", …");
    }
    language.format("ui.agent_tool.batch_of", &[("names", &list)])
}

/// A title an approval or permission card carries (`batch: solid.fillet, solid.chamfer (+2)`,
/// `mcp__solidcraft-app__execute`) with each command by its name. Any other title stays as it is.
pub fn card_title(language: Locale, title: &str) -> String {
    if let Some(rest) = title.strip_prefix("batch: ") {
        let (list, more) = match rest.rsplit_once(" (+") {
            Some((list, extra)) if extra.ends_with(')') => (list, true),
            _ => (rest, false),
        };
        let names: Vec<String> = list.split(", ").map(|c| name_in(language, c)).collect();
        return batch_of(language, &names, more || names.len() > BATCH_NAMES);
    }
    let one_id = !title.contains(char::is_whitespace) && (title.contains(['.', '_']) || is_mcp_id(title));
    if one_id { name_in(language, title) } else { title.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn english() -> Locale {
        crate::i18n::install();
        Locale::for_language("en")
    }

    #[test]
    fn the_server_is_taken_off_a_tool_name() {
        for raw in ["mcp__solidcraft-app__batch", "mcp.solidcraft-app.batch", "mcp:solidcraft/batch", "mcp.solidcraft_batch", "batch"] {
            assert_eq!(tool_of(raw), "batch", "{raw}");
        }
        assert_eq!(tool_of("solid.fillet"), "solid.fillet");
        assert!(is_mcp_id("mcp__solidcraft-app__batch") && !is_mcp_id("Read a.step"));
    }

    #[test]
    fn a_command_reads_as_its_label_however_it_is_named() {
        for raw in ["solid.fillet", "mcp.solidcraft_solid_fillet", "mcp__solidcraft-app__solid.fillet", "mcp.solidcraft_fillet"] {
            assert_eq!(name_in(english(), raw), "Fillet", "{raw}");
        }
    }

    #[test]
    fn a_method_without_a_command_reads_in_words() {
        assert_eq!(name_in(english(), "ui.prefs"), "Preferences");
        assert_eq!(name_in(english(), "ui.something_new"), "Something new");
        assert_eq!(name_in(english(), "mcp.solidcraft_do_thing"), "Do thing");
    }

    #[test]
    fn cards_name_their_commands() {
        assert_eq!(card_title(english(), "batch: solid.fillet, solid.chamfer (+3)"), "Batch: Fillet, Chamfer, …");
        assert_eq!(card_title(english(), "mcp__solidcraft-app__undo"), "Undo");
        assert_eq!(card_title(english(), "Read a.step"), "Read a.step");
    }

    #[test]
    fn a_batch_lists_its_first_commands() {
        let steps = ["Fillet", "Chamfer", "Hole", "Shell"].map(String::from);
        assert_eq!(batch_in(english(), &steps[..2]), "Batch: Fillet, Chamfer");
        assert_eq!(batch_in(english(), &steps), "Batch: Fillet, Chamfer, Hole, …");
    }

    /// Every tool the engine lists has a name a person can read, never its id.
    #[test]
    fn every_listed_tool_has_a_readable_name() {
        use solidcraft_engine::agent_tools::{MESSAGE_TOOLS, TOOL_COMMANDS};
        let ids = TOOL_COMMANDS.iter().map(|(t, _)| *t).chain(MESSAGE_TOOLS.iter().copied());
        for id in ids {
            for raw in [id.to_string(), format!("mcp__solidcraft-app__{id}"), format!("mcp.solidcraft_{id}")] {
                let shown = name_in(english(), &raw);
                assert!(!shown.trim().is_empty(), "{raw} has no name");
                assert!(!shown.contains("mcp") && !shown.contains("__") && !shown.contains('_'), "{raw} reads {shown:?}");
                assert_ne!(shown, id, "{raw} reads as its id");
            }
        }
        for tool in MESSAGE_TOOLS {
            let message = format!("ui.agent_tool.{tool}");
            assert_ne!(english().text(&message), message, "{message} is not in the catalog");
        }
        for (_, command) in TOOL_COMMANDS {
            assert!(solidcraft_engine::find_command(command).is_some(), "{command} is no command");
        }
        assert_eq!(name_in(english(), "mcp__solidcraft-app__undo"), "Undo");
    }
}
