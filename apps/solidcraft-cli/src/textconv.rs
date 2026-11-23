//! `textconv` prints a design file as text for `git diff` (#626). Git runs it on each side of a diff
//! when `.gitattributes` gives the design files the `solidcraft` diff driver and git config sets
//! `diff.solidcraft.textconv` (the user guide's "Designs in Git" section). The text comes from
//! [`solidcraft_engine::io::design_text`], which says what it shows and leaves out.
//!
//! Git stops the whole diff when this command fails. So a file it can read but not show prints one
//! line with its size and SHA-256 and exits 0. That covers a file over the design file limit, one
//! that is neither JSON nor a container, and a container that does not open. Only a file it cannot
//! read at all is an error. The text never names the file. For the old side of a
//! diff git hands textconv a temporary copy, and for the working tree side the file itself, whose
//! name may start with `--`. So the one argument is always a path, never an option.

use std::io::Write as _;

use solidcraft_engine::io::limits::{self, Kind};

/// The command's usage, from the same lines the top-level usage lists.
fn usage() -> String {
    crate::command_usage("textconv", &[])
}

pub fn run(args: &[String]) -> Result<(), String> {
    let [path] = args else { return Err(usage()) };
    let text = match limits::read(path, Kind::Design) {
        Ok(bytes) => solidcraft_engine::io::design_text(&bytes),
        // Over the design file limit. Its SHA-256, read a piece at a time, still shows a change.
        Err(e) if e.kind() == std::io::ErrorKind::InvalidData => std::fs::File::open(path)
            .and_then(|file| solidcraft_engine::io::large_file_text("too large to show, over the design file limit", file))
            .map_err(|e| format!("{path}: {e}"))?,
        Err(e) => return Err(format!("{path}: {e}")),
    };
    // A reader may stop early (`| head`), which is not an error.
    match std::io::stdout().lock().write_all(text.as_bytes()) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
        Err(e) => Err(format!("stdout: {e}")),
    }
}
