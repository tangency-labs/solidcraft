//! `solvecraft-cli`: the CLI's former name, kept for at least one release so existing scripts
//! and MCP client setups (`claude mcp add solvecraft -- solvecraft-cli mcp`) keep working.
//! Compatibility shim: it runs `solidcraft-cli` (next to this executable, else on PATH) with the
//! same arguments and standard streams, after a one-line deprecation note on stderr (stdout
//! stays clean for MCP's JSON-RPC), and exits with its status.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use std::process::{Command, ExitCode};

const TARGET: &str = "solidcraft-cli";

fn main() -> ExitCode {
    solidcraft_engine::note!("solvecraft-cli is deprecated and will be removed in a later release: use solidcraft-cli");
    let sibling = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(format!("{TARGET}{}", std::env::consts::EXE_SUFFIX))))
        .filter(|p| p.is_file());
    let program = sibling.map(|p| p.into_os_string()).unwrap_or_else(|| TARGET.into());
    match Command::new(&program).args(std::env::args_os().skip(1)).status() {
        Ok(status) => match status.code() {
            Some(code) => ExitCode::from(u8::try_from(code).unwrap_or(1)),
            None => ExitCode::FAILURE,
        },
        Err(e) => {
            solidcraft_engine::note!("solvecraft-cli: could not run {}: {e}", program.to_string_lossy());
            ExitCode::FAILURE
        }
    }
}
