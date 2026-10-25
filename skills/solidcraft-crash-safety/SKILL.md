---
name: solidcraft-crash-safety
description: "Use when writing or reviewing non-test Rust in SolidCraft, handling design files, command parameters or control-channel input, calling the kernel, or investigating a crash report. A crash loses someone's work, so this outranks feature work."
---

# Never crash

People trust SolidCraft with their designs. This outranks feature work.

## In non-test code
- No `unwrap()`, `expect()`, `panic!`, `unreachable!`, `todo!` or `unimplemented!`. Every production crate root carries `#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]`; `cargo xtask ci` runs clippy with `-D warnings`.
- No `unsafe` (`unsafe_code = "forbid"`). The exceptions, in the app crate and in `crates/threedx-client` (which deny it instead), have no safe API:
  - macOS only: `crates/threedx-client/src/macos.rs` (loads the 3Dconnexion driver's client framework at run time and calls through its function pointers), two functions in `apps/solidcraft/src/open_documents.rs` (macOS hands Finder files only to an Objective-C method), and `apps/solidcraft/src/native_crash_macos.rs` (the signal handler that records crashes inside native code, as `native_crash.rs` does on Windows)
  - Windows only: `apps/solidcraft/src/native_crash.rs` (the exception filter that records crashes inside native code such as graphics drivers), `apps/solidcraft/src/native_frame.rs` (the window procedure that answers the resize-border hit test of the window without OS decorations, and gives its class the icon the taskbar falls back on) and `apps/solidcraft/src/space_mouse/windows_hid.rs` (asks Plug and Play which HID interfaces are 3D mice, so no other device is sent anything)
- Errors are `Result<T, E>` through the crate's error type and `?`. An unfinished feature returns an error ("not supported yet: ..."); it never panics.
- Input-derived numbers are hostile (command parameters, design files, control-channel input): `get()` not `[i]`, finite checks, caps on sizes and loop counts.
- Everything that calls `truck` goes through `solidcraft_kernel::guard`, which turns a panic into `KernelError::Internal`. Only `crates/kernel` may use `truck`.
- Last resort: `Session::execute` runs every command under `catch_unwind`; an escaped panic restores the design and reports `EngineError::Internal`. Do not rely on it.
- Files are written with `crates/io/src/safe_write.rs` (temporary file, flush, one rename), so a crash leaves the old file or the new one.

## Proving a crash fix
- Every crash fix lands with a regression test. `hostile_params_never_panic` in `crates/engine/src/tests.rs` runs every command with junk parameters; a new command gets this for free.
- A crash report's `design.solidcraft` and `replay.json` are the test's starting point. Put the test in the area's existing test file (see `skills/solidcraft-tests/SKILL.md`).

## When someone says the app crashed
The desktop app writes every panic and every unclean exit to a local crash folder, with the design and the commands that reproduce it (`solidcraft_engine::crash`, `docs/crash-reports.md`). Look there first:
- `solidcraft-cli crashes`, `solidcraft-cli crashes show <id>`, `solidcraft-cli crashes replay <id>`
- the `diagnostics.crash_reports` command, or the MCP `crash_reports` tool

Crashes and data loss are taken as issues even while kernel work is paused (CONTRIBUTING.md, "Kernel work is paused").
