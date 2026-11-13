//! What the engine logs (`log::info!`, `log::debug!`: why a body has no display plan, why a save
//! left its model cache out), written to stderr. Off unless asked for, so the CLI's output and
//! the scripts reading it stay as they are: `--debug` shows SolidCraft's own debug messages (and
//! other crates' warnings, whose debug output is large), `RUST_LOG=error|warn|info|debug|trace`
//! sets one level for every crate.

use log::{LevelFilter, Log, Metadata, Record};

struct Stderr {
    /// The level for SolidCraft's crates.
    ours: LevelFilter,
    /// The level for every other crate.
    others: LevelFilter,
}

impl Log for Stderr {
    fn enabled(&self, m: &Metadata) -> bool {
        let target = m.target();
        let ours = target.starts_with("solidcraft") || target == "craft_plugin_wasm" || target.starts_with("craft_plugin_wasm::");
        m.level() <= if ours { self.ours } else { self.others }
    }

    fn log(&self, r: &Record) {
        if self.enabled(r.metadata()) {
            use std::io::Write as _;
            // A closed stderr is no reason to stop.
            let _ = writeln!(std::io::stderr().lock(), "[{} {}] {}", r.level().as_str().to_ascii_lowercase(), r.target(), r.args());
        }
    }

    fn flush(&self) {}
}

/// Removes `--debug` from `args`, wherever it is (every subcommand takes it), and installs the
/// logger when it or `RUST_LOG` asks for one.
pub fn init(args: &mut Vec<String>) {
    let before = args.len();
    args.retain(|a| a != "--debug");
    let debug = args.len() != before;
    let env = std::env::var("RUST_LOG").ok().and_then(|v| v.trim().parse::<LevelFilter>().ok());
    let (ours, others) = match (debug, env) {
        (false, Some(all)) => (all, all),
        (true, Some(all)) => (all.max(LevelFilter::Debug), all),
        (true, None) => (LevelFilter::Debug, LevelFilter::Warn),
        (false, None) => return,
    };
    // Installed once, for the life of the process.
    if log::set_logger(Box::leak(Box::new(Stderr { ours, others }))).is_ok() {
        log::set_max_level(ours.max(others));
    }
}
