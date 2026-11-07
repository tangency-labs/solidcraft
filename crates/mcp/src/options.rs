//! How the MCP server shapes what its tools return. Off by default: every option keeps the
//! behaviour the server had before it. Two places turn an option on: the server's own command
//! line and environment ([`Options::resolve`]), and, when the server is bridged to the app, the
//! app's Preferences (AI › MCP agents), which it reports as `mcp.options` ([`Options::from_app`]).
//! Either one is enough ([`Options::or`]).

/// The environment variable that turns on compact output: `1`, `true`, `yes` or `on`
/// (`0`, `false`, `no`, `off` or empty: off). `solidcraft-cli mcp --compact` does the same.
pub const COMPACT_ENV: &str = "SOLIDCRAFT_MCP_COMPACT";

/// The command-line flag that turns on compact output.
pub const COMPACT_FLAG: &str = "--compact";

/// The environment variable that turns on script mode (values as for [`COMPACT_ENV`]).
/// `solidcraft-cli mcp --scx` does the same.
pub const SCX_ENV: &str = "SOLIDCRAFT_MCP_SCX";

/// The command-line flag that turns on script mode.
pub const SCX_FLAG: &str = "--scx";

/// Output options of the MCP server.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Options {
    /// Compact output for agents that pay for every byte: `inspect_design` summarises sketches
    /// (full contents only for the sketches named in `sketches`) and takes a `bodies` filter, and
    /// tool results that are JSON are not pretty-printed. Off: `inspect_design` returns every
    /// sketch in full, and JSON is pretty-printed.
    pub compact: bool,
    /// Script mode (scx, docs/mcp.md, "Script mode"): the server lists three tools, `run`, `design` and `look`,
    /// and the agent builds with short Python scripts in a CAD vocabulary, run in a sandboxed
    /// interpreter with no file, network or process access. Off: the usual tools.
    pub scx: bool,
}

impl Options {
    /// The options from the command line's `--compact` and the value of [`COMPACT_ENV`]: either
    /// turns compact output on. An unrecognised value is an error, so a typo is not silently off.
    pub fn resolve(flag: bool, env: Option<&str>) -> Result<Self, String> {
        Ok(Self { compact: flag || env_on(COMPACT_ENV, env)?, scx: false })
    }

    /// [`Options::resolve`] with this process's command-line arguments (`args`, after the
    /// subcommand) and environment, script mode included (`--scx`, [`SCX_ENV`]).
    pub fn from_args_and_env(args: &[String]) -> Result<Self, String> {
        let mut o = Self::resolve(args.iter().any(|a| a == COMPACT_FLAG), std::env::var(COMPACT_ENV).ok().as_deref())?;
        o.scx = args.iter().any(|a| a == SCX_FLAG) || env_on(SCX_ENV, std::env::var(SCX_ENV).ok().as_deref())?;
        Ok(o)
    }

    /// The options the app reports in its `mcp.options` reply (`{"compact": bool, "scx": bool}`);
    /// a field missing or of another type is off.
    pub fn from_app(reply: &serde_json::Value) -> Self {
        let on = |k: &str| reply.get(k).and_then(serde_json::Value::as_bool).unwrap_or(false);
        Self { compact: on("compact"), scx: on("scx") }
    }

    /// Each option on when it is on in `self` or in `other`.
    pub fn or(self, other: Self) -> Self {
        Self { compact: self.compact || other.compact, scx: self.scx || other.scx }
    }

    /// Tool JSON as text: compact, or pretty-printed (the default).
    pub(crate) fn json_text(self, v: &serde_json::Value) -> String {
        if self.compact { v.to_string() } else { serde_json::to_string_pretty(v).unwrap_or_default() }
    }
}

/// Whether the environment variable `name`, with value `v`, turns its option on.
fn env_on(name: &str, v: Option<&str>) -> Result<bool, String> {
    match v.map(|v| v.trim().to_ascii_lowercase()).as_deref() {
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
        assert_eq!(Options::default(), Options { compact: false, scx: false });
        assert_eq!(Options::resolve(false, None), Ok(Options { compact: false, scx: false }));
        for v in ["", "0", "false", "No", " off "] {
            assert_eq!(Options::resolve(false, Some(v)), Ok(Options { compact: false, scx: false }), "{v:?}");
        }
        for v in ["1", "true", "YES", "on"] {
            assert_eq!(Options::resolve(false, Some(v)), Ok(Options { compact: true, scx: false }), "{v:?}");
        }
        assert_eq!(Options::resolve(true, None), Ok(Options { compact: true, scx: false }));
        assert_eq!(Options::resolve(true, Some("0")), Ok(Options { compact: true, scx: false }), "the flag wins");
        assert!(Options::resolve(false, Some("maybe")).is_err_and(|e| e.contains(COMPACT_ENV)));
    }

    #[test]
    fn the_app_or_the_command_line_turns_it_on() {
        let off = Options::default();
        let on = Options { compact: true, scx: false };
        assert_eq!(Options::from_app(&serde_json::json!({"compact": true})), on);
        for reply in [serde_json::json!({}), serde_json::json!({"compact": "yes"}), serde_json::json!(null), serde_json::json!({"compact": false})] {
            assert_eq!(Options::from_app(&reply), off, "{reply}");
        }
        assert_eq!(off.or(off), off);
        assert_eq!(off.or(on), on);
        assert_eq!(on.or(off), on);
        let scx = Options { compact: false, scx: true };
        assert_eq!(Options::from_app(&serde_json::json!({"scx": true})), scx);
        assert_eq!(Options::from_app(&serde_json::json!({"scx": 1})), off);
        assert_eq!(on.or(scx), Options { compact: true, scx: true });
    }
}
