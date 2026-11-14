//! What an agent's MCP tools are called to a person: the command a tool stands for, or (for a
//! tool that is no command) a catalog message `ui.agent_tool.<tool>`. The UI reads both
//! (`solidcraft_ui_egui::agent_label`); the MCP server's tests check that every tool it offers is
//! listed here.

/// Tools that are one command: the tool reads as that command's label.
pub const TOOL_COMMANDS: &[(&str, &str)] = &[
    ("crash_reports", "diagnostics.crash_reports"),
    ("export", "file.export"),
    ("inspect_design", "document.inspect"),
    ("measure", "inspect.measure"),
    ("new_design", "file.new"),
    ("open", "doc.open"),
    ("redo", "edit.redo"),
    ("save", "file.save"),
    ("set_parameter", "parameters.change"),
    ("undo", "edit.undo"),
];

/// Tools that are no command: each has a message `ui.agent_tool.<tool>`.
pub const MESSAGE_TOOLS: &[&str] = &[
    "batch",
    "body_topology",
    "cancel",
    "check_parameters",
    "compare_picture",
    "describe_command",
    "execute",
    "list_commands",
    "pending",
    "running",
    "screenshot",
    "skill_reference",
];

/// The command a tool stands for, when it is one.
pub fn tool_command(tool: &str) -> Option<&'static str> {
    TOOL_COMMANDS.iter().find(|(t, _)| *t == tool).map(|(_, command)| *command)
}
