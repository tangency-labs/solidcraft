//! `textconv`: a design file as text, for `git diff` (#626). Git runs it on each side of a diff
//! when `.gitattributes` gives the design files the `solidcraft` diff driver and git config sets
//! `diff.solidcraft.textconv` (the user guide's "Designs in Git" section). The text comes from
//! [`solidcraft_engine::io::design_text`], which says what it shows and leaves out.
//!
//! Git stops the whole diff when this command fails, so a file it can read but not show (too large,
//! not a design, a damaged container) prints one line and exits 0. Only a file it cannot read at all
//! is an error. The text never names the file, because git hands textconv a temporary copy.

use std::io::Write as _;

use solidcraft_engine::io::limits::{self, Kind};

/// The command's usage, from the same lines the top-level usage lists.
fn usage() -> String {
    crate::command_usage("textconv", &[])
}

pub fn run(args: &[String]) -> Result<(), String> {
    crate::known_args("textconv", args, &[], &[], true)?;
    let [path] = args else { return Err(usage()) };
    let text = match limits::read(path, Kind::Design) {
        Ok(bytes) => solidcraft_engine::io::design_text(&bytes),
        // Over the design file limit: one line with the size, so git still shows that it changed.
        Err(e) if e.kind() == std::io::ErrorKind::InvalidData => {
            let size = solidcraft_engine::io::vfs::len(path).map_or_else(|_| "unknown size".to_string(), |n| format!("{n} bytes"));
            format!("# not shown ({size}): {e}\n")
        }
        Err(e) => return Err(format!("{path}: {e}")),
    };
    // A reader may stop early (`| head`): that is not an error.
    match std::io::stdout().lock().write_all(text.as_bytes()) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
        Err(e) => Err(format!("stdout: {e}")),
    }
}
