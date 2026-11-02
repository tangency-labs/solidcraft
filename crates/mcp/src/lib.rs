//! SolidCraft MCP server.
//!
//! JSON-RPC 2.0 over stdio (newline-delimited), implementing the MCP lifecycle, tools and
//! resources. Tools map onto the control-channel methods (`engine.execute`, `document.inspect`,
//! `ui.render`…, see `docs/control-protocol.md`), served either by an in-process headless
//! [`Headless`] session or by a running app through its loopback control port ([`Remote`]), so a
//! person can watch the agent model. Every modelling step is an engine command; tools never open
//! dialogs.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod backend;
mod server;
mod skill_reference;
mod tools;

pub use backend::{Auto, Backend, Headless, Remote};
pub use server::{PROTOCOL_VERSION, SUPPORTED_VERSIONS, Server};
pub use tools::{call_tool, tool_definitions};

use std::path::PathBuf;

use solidcraft_engine::io::vfs::{self, PathScope};

/// The folders `solidcraft-cli mcp` lets its agent read, write and delete in by default: the
/// app's own recovery folder, this server's private scratch folder (screenshots), and each
/// `allowed` folder (`--allow-path`). The open design's folder joins on every call when it is a
/// safe one (`vfs::design_folder_refusal`; the server adds it), and stays for the rest of the run
/// like the folder of every design a call opens or saves (#7181). The working folder, the home
/// folder and anything above it never count by themselves, so `~/.ssh`, `~/.bashrc` and other
/// dotfiles are out of reach unless a folder holding them is allowed on purpose.
pub fn agent_scope(allowed: impl IntoIterator<Item = PathBuf>) -> PathScope {
    agent_scope_with(solidcraft_engine::recovery::default_dir(), allowed)
}

/// [`agent_scope`] with the recovery folder given (tests).
pub fn agent_scope_with(recovery: Option<PathBuf>, allowed: impl IntoIterator<Item = PathBuf>) -> PathScope {
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Some(d) = recovery
        && std::fs::create_dir_all(&d).is_ok()
    {
        roots.push(d);
    }
    if let Some(d) = scratch_dir() {
        roots.push(d);
    }
    roots.extend(allowed);
    PathScope::new(roots)
}

static SCRATCH_DIR: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();

/// This server's own folder for screenshots it reads back and deletes (`screenshot` without a
/// `path`): a new `<temp>/solidcraft-mcp-<pid>-<n>` made when the path scope is built, or on the
/// first screenshot without a scope. Never reuse an existing folder, which could be someone
/// else's. It is private to the user on Unix. `<temp>` is spelled with long names (`vfs::temp_dir`),
/// since the scope refuses short (8.3) ones.
pub fn scratch_dir() -> Option<PathBuf> {
    SCRATCH_DIR
        .get_or_init(|| {
            let b = std::fs::DirBuilder::new();
            #[cfg(unix)]
            let mut b = b;
            #[cfg(unix)]
            std::os::unix::fs::DirBuilderExt::mode(&mut b, 0o700);
            let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
            (0..16u32).find_map(|i| {
                let d = vfs::temp_dir().join(format!("solidcraft-mcp-{}-{:x}", std::process::id(), seed.wrapping_add(i)));
                b.create(&d).ok().map(|()| d)
            })
        })
        .clone()
}

/// Best-effort cleanup at process shutdown, after all MCP activity has stopped. Only remove
/// this process's empty scratch folder: saved files, other processes' folders and the recovery
/// folder must survive. Looking up the cached path never creates a folder just to clean it up.
pub fn cleanup_scratch_dir() {
    if let Some(Some(dir)) = SCRATCH_DIR.get() {
        let _ = std::fs::remove_dir(dir);
    }
}

#[cfg(test)]
mod tests;
