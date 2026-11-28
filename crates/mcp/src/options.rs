//! How the MCP server shapes what its tools return. Off by default: every option keeps the
//! behaviour the server had before it, except the tool listing of `solidcraft-cli mcp`, which
//! lists the core tools unless asked for all of them ([`ALL_TOOLS_FLAG`]). Two places turn an option on: the server's own command
//! line and environment ([`Options::resolve`]), and, when the server is bridged to the app, the
//! app's Preferences (AI › MCP agents), which it reports as `mcp.options` ([`Options::from_app`]).
//! Either one is enough ([`Options::or`]).

/// The environment variable that turns on compact output: `1`, `true`, `yes` or `on`
/// (`0`, `false`, `no`, `off` or empty: off). `solidcraft-cli mcp --compact` does the same.
pub const COMPACT_ENV: &str = "SOLIDCRAFT_MCP_COMPACT";

/// The command-line flag that turns on compact output.
pub const COMPACT_FLAG: &str = "--compact";

/// The command-line flag that adds `structuredContent` to every tool result, for programs that
/// call the tools (the Python client): `{"result": the reply as JSON (or its text when it is
/// not JSON), "notes": [the text blocks added after it]}`. Chat clients leave it off, since some
/// show the model both the text and the structured copy.
pub const STRUCTURED_FLAG: &str = "--structured";

/// The command-line flag that lists every tool, as the server did before `find_commands` and
/// `call_command`: without it `solidcraft-cli mcp` lists only [`crate::CORE_TOOLS`], which reach
/// every command, and answers a call to any other tool by name as before.
pub const ALL_TOOLS_FLAG: &str = "--all-tools";

/// The environment variable that does what [`ALL_TOOLS_FLAG`] does: `1`, `true`, `yes` or `on`.
pub const ALL_TOOLS_ENV: &str = "SOLIDCRAFT_MCP_ALL_TOOLS";

/// Output options of the MCP server.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Options {
    /// Compact output for agents that pay for every byte: `inspect_design` summarises sketches
    /// (full contents only for the sketches named in `sketches`) and takes a `bodies` filter, and
    /// tool results that are JSON are not pretty-printed. Off: `inspect_design` returns every
    /// sketch in full, and JSON is pretty-printed.
    pub compact: bool,
    /// Every tool result also carries its reply as `structuredContent` ([`STRUCTURED_FLAG`]).
    /// Off: only errors with a structured reason carry it, as before.
    pub structured: bool,
    /// `tools/list` lists only [`crate::CORE_TOOLS`] (and plugin tools), and the instructions
    /// speak of those; every tool still answers when called by name. Off (the library's default,
    /// and `solidcraft-cli mcp --all-tools`): every tool is listed.
    pub core_tools: bool,
}

impl Options {
    /// The options from the command line's `--compact` and the value of [`COMPACT_ENV`]: either
    /// turns compact output on. An unrecognised value is an error, so a typo is not silently off.
    pub fn resolve(flag: bool, env: Option<&str>) -> Result<Self, String> {
        Ok(Self { compact: flag || switch(COMPACT_ENV, env)?, ..Self::default() })
    }

    /// [`Options::resolve`] with this process's command-line arguments (`args`, after the
    /// subcommand) and environment.
    pub fn from_args_and_env(args: &[String]) -> Result<Self, String> {
        let structured = args.iter().any(|a| a == STRUCTURED_FLAG);
        let all_tools = args.iter().any(|a| a == ALL_TOOLS_FLAG) || switch(ALL_TOOLS_ENV, std::env::var(ALL_TOOLS_ENV).ok().as_deref())?;
        Self::resolve(args.iter().any(|a| a == COMPACT_FLAG), std::env::var(COMPACT_ENV).ok().as_deref()).map(|o| Self {
            structured,
            core_tools: !all_tools,
            ..o
        })
    }

    /// The options the app reports in its `mcp.options` reply (`{"compact": bool}`); a field
    /// missing or of another type is off. Structured results are the calling program's choice,
    /// never the app's.
    pub fn from_app(reply: &serde_json::Value) -> Self {
        Self { compact: reply.get("compact").and_then(serde_json::Value::as_bool).unwrap_or(false), ..Self::default() }
    }

    /// Each option on when it is on in `self` or in `other`.
    pub fn or(self, other: Self) -> Self {
        Self {
            compact: self.compact || other.compact,
            structured: self.structured || other.structured,
            core_tools: self.core_tools || other.core_tools,
        }
    }

    /// Tool JSON as text: compact, or pretty-printed (the default).
    pub(crate) fn json_text(self, v: &serde_json::Value) -> String {
        if self.compact { v.to_string() } else { serde_json::to_string_pretty(v).unwrap_or_default() }
    }
}

/// An on/off environment variable `name` with value `env`: unset or empty is off, and an
/// unrecognised value is an error, so a typo is not silently off.
fn switch(name: &str, env: Option<&str>) -> Result<bool, String> {
    match env.map(|v| v.trim().to_ascii_lowercase()).as_deref() {
        None | Some("" | "0" | "false" | "no" | "off") => Ok(false),
        Some("1" | "true" | "yes" | "on") => Ok(true),
        Some(other) => Err(format!("{name} must be 1 or 0 (true/false, yes/no, on/off), not `{other}`")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_unless_asked() {
        assert_eq!(Options::default(), Options::default());
        assert_eq!(Options::resolve(false, None), Ok(Options::default()));
        for v in ["", "0", "false", "No", " off "] {
            assert_eq!(Options::resolve(false, Some(v)), Ok(Options::default()), "{v:?}");
        }
        for v in ["1", "true", "YES", "on"] {
            assert_eq!(Options::resolve(false, Some(v)), Ok(Options { compact: true, ..Options::default() }), "{v:?}");
        }
        assert_eq!(Options::resolve(true, None), Ok(Options { compact: true, ..Options::default() }));
        assert_eq!(Options::resolve(true, Some("0")), Ok(Options { compact: true, ..Options::default() }), "the flag wins");
        assert!(Options::resolve(false, Some("maybe")).is_err_and(|e| e.contains(COMPACT_ENV)));
    }

    #[test]
    fn structured_comes_only_from_the_command_line() {
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let on = Options::from_args_and_env(&args(&["--headless", STRUCTURED_FLAG])).unwrap();
        assert!(on.structured);
        assert!(!Options::from_args_and_env(&args(&["--headless"])).unwrap().structured);
        assert!(!Options::from_app(&serde_json::json!({"compact": true, "structured": true})).structured);
        let off = Options::default();
        assert!(off.or(on).structured && on.or(off).structured);
    }

    #[test]
    fn the_command_line_lists_the_core_tools_unless_asked_for_all() {
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(!Options::default().core_tools, "the library lists every tool");
        assert!(Options::from_args_and_env(&args(&["--headless"])).unwrap().core_tools || std::env::var(ALL_TOOLS_ENV).is_ok());
        assert!(!Options::from_args_and_env(&args(&["--headless", ALL_TOOLS_FLAG])).unwrap().core_tools);
        assert!(!Options::from_app(&serde_json::json!({"core_tools": true})).core_tools, "never the app's choice");
        assert_eq!(switch(ALL_TOOLS_ENV, Some("yes")), Ok(true));
        assert!(switch(ALL_TOOLS_ENV, Some("maybe")).is_err_and(|e| e.contains(ALL_TOOLS_ENV)));
    }

    #[test]
    fn the_app_or_the_command_line_turns_it_on() {
        let off = Options::default();
        let on = Options { compact: true, ..Options::default() };
        assert_eq!(Options::from_app(&serde_json::json!({"compact": true})), on);
        for reply in [serde_json::json!({}), serde_json::json!({"compact": "yes"}), serde_json::json!(null), serde_json::json!({"compact": false})] {
            assert_eq!(Options::from_app(&reply), off, "{reply}");
        }
        assert_eq!(off.or(off), off);
        assert_eq!(off.or(on), on);
        assert_eq!(on.or(off), on);
    }
}
