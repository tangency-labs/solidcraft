//! How the MCP server shapes what its tools return. Off by default: every option keeps the
//! behaviour the server had before it. [`Options::resolve`] is the one place that decides, so a
//! later source (an app Preferences setting sent over the control channel, say) only changes
//! what it is given.

/// The environment variable that turns on compact output: `1`, `true`, `yes` or `on`
/// (`0`, `false`, `no`, `off` or empty: off). `solidcraft-cli mcp --compact` does the same.
pub const COMPACT_ENV: &str = "SOLIDCRAFT_MCP_COMPACT";

/// The command-line flag that turns on compact output.
pub const COMPACT_FLAG: &str = "--compact";

/// Output options of the MCP server.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Options {
    /// Compact output for agents that pay for every byte: `inspect_design` summarises sketches
    /// (full contents only for the sketches named in `sketches`) and takes a `bodies` filter, and
    /// tool results that are JSON are not pretty-printed. Off: `inspect_design` returns every
    /// sketch in full, and JSON is pretty-printed.
    pub compact: bool,
}

impl Options {
    /// The options from the command line's `--compact` and the value of [`COMPACT_ENV`]: either
    /// turns compact output on. An unrecognised value is an error, so a typo is not silently off.
    pub fn resolve(flag: bool, env: Option<&str>) -> Result<Self, String> {
        let from_env = match env.map(|v| v.trim().to_ascii_lowercase()).as_deref() {
            None | Some("" | "0" | "false" | "no" | "off") => false,
            Some("1" | "true" | "yes" | "on") => true,
            Some(other) => return Err(format!("{COMPACT_ENV} must be 1 or 0 (true/false, yes/no, on/off), not `{other}`")),
        };
        Ok(Self { compact: flag || from_env })
    }

    /// [`Options::resolve`] with this process's command-line arguments (`args`, after the
    /// subcommand) and environment.
    pub fn from_args_and_env(args: &[String]) -> Result<Self, String> {
        Self::resolve(args.iter().any(|a| a == COMPACT_FLAG), std::env::var(COMPACT_ENV).ok().as_deref())
    }

    /// Tool JSON as text: compact, or pretty-printed (the default).
    pub(crate) fn json_text(self, v: &serde_json::Value) -> String {
        if self.compact { v.to_string() } else { serde_json::to_string_pretty(v).unwrap_or_default() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_unless_asked() {
        assert_eq!(Options::default(), Options { compact: false });
        assert_eq!(Options::resolve(false, None), Ok(Options { compact: false }));
        for v in ["", "0", "false", "No", " off "] {
            assert_eq!(Options::resolve(false, Some(v)), Ok(Options { compact: false }), "{v:?}");
        }
        for v in ["1", "true", "YES", "on"] {
            assert_eq!(Options::resolve(false, Some(v)), Ok(Options { compact: true }), "{v:?}");
        }
        assert_eq!(Options::resolve(true, None), Ok(Options { compact: true }));
        assert_eq!(Options::resolve(true, Some("0")), Ok(Options { compact: true }), "the flag wins");
        assert!(Options::resolve(false, Some("maybe")).is_err_and(|e| e.contains(COMPACT_ENV)));
    }
}
