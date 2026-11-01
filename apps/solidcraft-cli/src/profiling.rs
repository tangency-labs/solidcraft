//! `--profile` and `--profile-out FILE` for `run`, `eval` and `bench-open` (and
//! `SOLIDCRAFT_PROFILE=1` for any command): record where the time goes, SMLib's calls to the
//! commands, print the table to stderr when the command ends (stdout stays the command's JSON)
//! and write the spans as a Chrome trace for chrome://tracing or Perfetto.

use solidcraft_engine::profile;

/// Rows the table prints.
const TOP: usize = 30;

/// A recording that prints its table when finished.
pub struct Profiling {
    trace: Option<String>,
}

/// Start recording when `args` (after the command) ask for it, or `SOLIDCRAFT_PROFILE` does.
pub fn start(args: &[String]) -> Result<Option<Profiling>, String> {
    let trace = match args.iter().position(|a| a == "--profile-out") {
        Some(i) => match args.get(i + 1) {
            Some(f) if !f.starts_with("--") => Some(f.clone()),
            _ => return Err("--profile-out needs a FILE".into()),
        },
        None => None,
    };
    let asked = trace.is_some() || args.iter().any(|a| a == "--profile");
    if !asked && !solidcraft_engine::timing::profile_from_env() {
        return Ok(None);
    }
    if !profile::COMPILED {
        return Err("this solidcraft-cli was built without the profiler (its `profile` feature)".into());
    }
    profile::reset();
    profile::set_enabled(true);
    Ok(Some(Profiling { trace }))
}

impl Profiling {
    /// Stop recording, print the table on stderr and write the trace.
    pub fn finish(self) -> Result<(), String> {
        profile::set_enabled(false);
        eprint!("{}", profile::report().table(TOP));
        if let Some(path) = self.trace {
            let mut bytes = Vec::new();
            profile::write_chrome_trace(&mut bytes).map_err(|e| format!("{path}: {e}"))?;
            std::fs::write(&path, bytes).map_err(|e| format!("{path}: {e}"))?;
            eprintln!("profile: trace written to {path} (open it in chrome://tracing or ui.perfetto.dev)");
        }
        Ok(())
    }
}
